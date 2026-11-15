//! Headless batch entry points. The graphical viewport is the product; these
//! commands only automate the same kernel for background processing and
//! release verification. Nothing here computes geometry of its own: every
//! contour and cap comes from `section_kernel::build_section`.

use crate::model::{Scene, WorldScene};
use crate::section::{build_section, SectionRequest, SectionResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// ---------------------------------------------------------------------------
// Batch job schema (archisection.batch/1)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchJob {
    #[serde(default)]
    schema: Option<String>,
    /// Model document, or a path to one. Exactly one must be present.
    #[serde(default)]
    scene_path: Option<PathBuf>,
    #[serde(default)]
    scene: Option<Value>,
    cuts: Vec<BatchCut>,
    /// When true, an `openChain`/`noClosedContours`/error diagnostic fails the
    /// job. Defaults to false: an open shell is a valid, reported result.
    #[serde(default)]
    require_closed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchCut {
    id: String,
    normal: [f64; 3],
    #[serde(default)]
    offset: Option<f64>,
    #[serde(default)]
    origin: Option<[f64; 3]>,
    #[serde(default)]
    include_segments: Option<bool>,
    /// Output file for the full result document. Relative paths resolve
    /// against the job's output directory.
    #[serde(default)]
    output: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchReport {
    schema: String,
    job: String,
    scene: String,
    result_count: usize,
    results: Vec<CutReport>,
    /// Non-empty only when a cut failed; the process also exits non-zero.
    failures: Vec<CutFailure>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CutReport {
    id: String,
    output: PathBuf,
    result_id: String,
    result_path_rel: String,
    closed_contours: usize,
    open_chains: usize,
    holes: usize,
    caps: usize,
    diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CutFailure {
    id: String,
    error: String,
}

/// Manifest written next to every result so a consumer can confirm the file
/// is exactly the kernel output the batch run produced and still reloads.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultManifest {
    schema: String,
    result_id: String,
    result_sha256: String,
    result_bytes: u64,
    plane: Value,
    tolerance_linear: f64,
    contour_count: usize,
    closed_contour_count: usize,
    cap_count: usize,
}

// ---------------------------------------------------------------------------
// Sub-commands
// ---------------------------------------------------------------------------

pub fn run(args: &[String]) -> ExitCode {
    let Some(cmd) = args.first() else {
        print_usage();
        return ExitCode::from(2);
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "serve" => serve(rest),
        "section" => section(rest),
        "batch" => batch(rest),
        "verify" => verify(rest),
        "lock" => lock(rest),
        "-h" | "--help" | "help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown sub-command: {other}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    eprintln!(
        "section-server — archisection geometry tool\n\
         \n\
         USAGE:\n\
           section-server serve [--port N] [--static-dir DIR]\n\
           section-server section --scene model.json --normal 0,0,1 --offset 1.5 --out result.json\n\
           section-server batch --job job.json --out-dir release-dir [--require-closed]\n\
           section-server verify --result result.json [--require-closed]\n\
           section-server lock --repo-root . --out release/release-lock.json\n\
         \n\
         The graphical viewport (./start.sh, then open the printed URL) is the\n\
         primary model-sectioning interface. These CLI commands only automate\n\
         the same kernel for background and release-pipeline work."
    );
}

// --- serve: hand control back to the HTTP/UI server -----------------------

fn serve(args: &[String]) -> ExitCode {
    // The HTTP server lives in the binary crate; its entry point is injected
    // at build time via the bin's `with_serve` initialiser.
    match SERVE_FN.get() {
        Some(f) => f(args),
        None => {
            eprintln!("serve is only available from the section-server binary");
            ExitCode::FAILURE
        }
    }
}

/// Signature of the binary's HTTP launcher.
pub type ServeFn = fn(&[String]) -> ExitCode;

static SERVE_FN: std::sync::OnceLock<ServeFn> = std::sync::OnceLock::new();

/// Wired by `main.rs` so the library's `serve` sub-command starts the full
/// API + graphical UI server without the library depending on axum.
pub fn install_serve(f: ServeFn) {
    let _ = SERVE_FN.set(f);
}

// --- single cut ------------------------------------------------------------

fn section(args: &[String]) -> ExitCode {
    let mut scene_path: Option<PathBuf> = None;
    let mut normal = [0.0f64, 0.0, 1.0];
    let mut offset: Option<f64> = None;
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let value = match args.get(i + 1) {
            Some(v) => v,
            None => {
                eprintln!("missing value for {arg}");
                return ExitCode::from(2);
            }
        };
        match arg.as_str() {
            "--scene" => scene_path = Some(PathBuf::from(value)),
            "--normal" => {
                let parts: Vec<f64> = value.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                if parts.len() != 3 {
                    eprintln!("--normal must be three comma-separated numbers, got {value}");
                    return ExitCode::from(2);
                }
                normal = [parts[0], parts[1], parts[2]];
            }
            "--offset" => offset = value.parse().ok(),
            "--out" => out = Some(PathBuf::from(value)),
            other => {
                eprintln!("unknown section flag: {other}");
                return ExitCode::from(2);
            }
        }
        i += 2;
    }
    let Some(scene_path) = scene_path else {
        eprintln!("--scene is required");
        return ExitCode::from(2);
    };
    let Some(offset) = offset else {
        eprintln!("--offset is required");
        return ExitCode::from(2);
    };
    let world = match load_world(&scene_path) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("scene load failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let req = SectionRequest {
        normal,
        offset: Some(offset),
        origin: None,
        include_segments: Some(true),
    };
    let result = match build_section(&req, &world) {
        Ok(r) => r,
        Err(errs) => {
            for e in &errs {
                eprintln!("section failed [{}]: {}", e.code, e.message);
            }
            return ExitCode::FAILURE;
        }
    };
    let json = match serde_json::to_string_pretty(&result) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("result serialization failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(path) = &out {
        if let Err(e) = write_atomic(path, json.as_bytes()) {
            eprintln!("could not write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
        println!(
            "wrote {} ({} contours, {} caps, result_id={})",
            path.display(),
            result.contours.len(),
            result.caps.len(),
            result.result_id
        );
    } else {
        println!("{json}");
    }
    ExitCode::SUCCESS
}

// --- batch -----------------------------------------------------------------

fn batch(args: &[String]) -> ExitCode {
    let mut job_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut force_require_closed = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--job" => {
                job_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--out-dir" => {
                out_dir = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--require-closed" => {
                force_require_closed = true;
                i += 1;
            }
            other => {
                eprintln!("unknown batch flag: {other}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(job_path), Some(out_dir)) = (job_path, out_dir) else {
        eprintln!("batch requires --job and --out-dir");
        return ExitCode::from(2);
    };

    let job_text = match fs::read_to_string(&job_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read job {}: {e}", job_path.display());
            return ExitCode::FAILURE;
        }
    };
    let mut job: BatchJob = match serde_json::from_str(&job_text) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("job document is invalid: {e}");
            return ExitCode::FAILURE;
        }
    };
    if force_require_closed {
        job.require_closed = true;
    }
    if let Some(tag) = &job.schema {
        if tag != "archisection.batch/1" {
            eprintln!("unsupported batch job schema: {tag} (expected archisection.batch/1)");
            return ExitCode::FAILURE;
        }
    }
    if job.cuts.is_empty() {
        eprintln!("job contains no cuts");
        return ExitCode::FAILURE;
    }

    // Resolve the model once. A scene referenced by path keeps large models
    // out of the job document; an inline scene is written beside the outputs
    // so the run is self-contained and reloadable.
    let world = match load_job_scene(&job, &job_path, &out_dir) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("scene load failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = fs::create_dir_all(&out_dir) {
        eprintln!("cannot create output dir {}: {e}", out_dir.display());
        return ExitCode::FAILURE;
    }

    let mut report = BatchReport {
        schema: "archisection.batch-report/1".into(),
        job: job_path.display().to_string(),
        scene: world.name.clone(),
        result_count: 0,
        results: Vec::new(),
        failures: Vec::new(),
    };

    for cut in &job.cuts {
        match run_cut(&world, cut, &out_dir, job.require_closed) {
            Ok(entry) => report.results.push(entry),
            Err(msg) => {
                eprintln!("cut '{}' failed: {msg}", cut.id);
                report.failures.push(CutFailure {
                    id: cut.id.clone(),
                    error: msg,
                });
            }
        }
    }
    report.result_count = report.results.len();

    let report_path = out_dir.join("batch-report.json");
    let report_json = serde_json::to_string_pretty(&report).unwrap_or_default();
    if let Err(e) = write_atomic(&report_path, report_json.as_bytes()) {
        eprintln!("could not write batch report: {e}");
        return ExitCode::FAILURE;
    }

    if !report.failures.is_empty() {
        eprintln!(
            "batch finished with {} failure(s); no promotion should consume this directory",
            report.failures.len()
        );
        ExitCode::FAILURE
    } else {
        println!(
            "batch ok: {} cut(s), report {}",
            report.result_count,
            report_path.display()
        );
        ExitCode::SUCCESS
    }
}

fn run_cut(
    world: &WorldScene,
    cut: &BatchCut,
    out_dir: &Path,
    require_closed: bool,
) -> Result<CutReport, String> {
    let req = SectionRequest {
        normal: cut.normal,
        offset: cut.offset,
        origin: cut.origin,
        include_segments: cut.include_segments.or(Some(true)),
    };
    let result = build_section(&req, world).map_err(|errs| {
        errs.iter()
            .map(|e| format!("[{}] {}", e.code, e.message))
            .collect::<Vec<_>>()
            .join("; ")
    })?;

    // Reject runs that hide an unresolved section behind exit code zero when
    // the job explicitly needs closed geometry.
    if require_closed {
        let blocking: Vec<&str> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d.code.as_str(), "openChain" | "noClosedContours"))
            .map(|d| d.code.as_str())
            .collect();
        if !blocking.is_empty() {
            return Err(format!(
                "requireClosed set but section reports {}",
                blocking.join(", ")
            ));
        }
        if result.contours.iter().all(|c| !c.closed) {
            return Err("requireClosed set but no closed contour was produced".into());
        }
    }

    let json = serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?;
    let target = cut
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("{}.json", safe_file_name(&cut.id))));
    let dest = if target.is_absolute() {
        target
    } else {
        out_dir.join(target)
    };
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    write_atomic(&dest, &json).map_err(|e| format!("write {}: {e}", dest.display()))?;

    let hash = sha256_hex(&json);
    let manifest = ResultManifest {
        schema: "archisection.result-manifest/1".into(),
        result_id: result.result_id.clone(),
        result_sha256: hash,
        result_bytes: json.len() as u64,
        plane: serde_json::to_value(&result.plane).map_err(|e| e.to_string())?,
        tolerance_linear: result.tolerance.linear,
        contour_count: result.contours.len(),
        closed_contour_count: result.stats.closed_contours,
        cap_count: result.caps.len(),
    };
    let manifest_path = dest.with_extension("manifest.json");
    let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    write_atomic(&manifest_path, &manifest_json).map_err(|e| e.to_string())?;

    let rel = dest
        .strip_prefix(out_dir)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| dest.display().to_string());
    Ok(CutReport {
        id: cut.id.clone(),
        output: dest.clone(),
        result_id: result.result_id,
        result_path_rel: rel,
        closed_contours: result.stats.closed_contours,
        open_chains: result.stats.open_chains,
        holes: result.stats.holes,
        caps: result.caps.len(),
        diagnostics: result.diagnostics.iter().map(|d| d.code.clone()).collect(),
    })
}

// --- verify ----------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ResultEnvelope {
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    result: Option<SectionResult>,
    #[serde(default, rename = "schema")]
    _schema: Option<String>,
}

fn verify(args: &[String]) -> ExitCode {
    let mut result_path: Option<PathBuf> = None;
    let mut require_closed = false;
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--result" => {
                result_path = Some(PathBuf::from(&args[idx + 1]));
                idx += 2;
            }
            "--require-closed" => {
                require_closed = true;
                idx += 1;
            }
            other => {
                eprintln!("unknown verify flag: {other}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(path) = result_path else {
        eprintln!("verify requires --result");
        return ExitCode::from(2);
    };
    match verify_file(&path, require_closed) {
        Ok(summary) => {
            println!("verify ok: {summary}");
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("verify failed for {}: {msg}", path.display());
            ExitCode::FAILURE
        }
    }
}

/// Reloads a produced result document and checks every geometric invariant the
/// release relies on: parseability, closed rings, plane incidence, cap ring
/// closure and consistent triangle counts.
fn verify_file(path: &Path, require_closed: bool) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("invalid JSON: {e}"))?;

    // Accept both a bare result and an {"ok":true,"result":{...}} envelope.
    let result: SectionResult = if value.get("result").is_some() {
        let envelope: ResultEnvelope =
            serde_json::from_value(value).map_err(|e| format!("invalid envelope: {e}"))?;
        if envelope.ok == Some(false) {
            return Err("document is an error response, not a result".into());
        }
        envelope
            .result
            .ok_or_else(|| "envelope has no result".to_string())?
    } else {
        serde_json::from_value(value).map_err(|e| format!("invalid result document: {e}"))?
    };

    let tol = result.tolerance.linear;
    if !(tol.is_finite() && tol > 0.0) {
        return Err(format!("tolerance.linear must be positive, got {tol}"));
    }
    let n = result.plane.normal;
    let nlen = (n[0].powi(2) + n[1].powi(2) + n[2].powi(2)).sqrt();
    if (nlen - 1.0).abs() > 1e-9 {
        return Err(format!("plane normal is not unit length (|n|={nlen})"));
    }

    let mut closed = 0usize;
    for c in &result.contours {
        if c.points2d.len() != c.points3d.len() {
            return Err(format!("contour {} mixes 2D/3D point counts", c.id));
        }
        if c.closed {
            closed += 1;
            if c.points3d.len() < 3 {
                return Err(format!("closed contour {} has fewer than 3 points", c.id));
            }
            for p in &c.points3d {
                let dist = n[0] * p[0] + n[1] * p[1] + n[2] * p[2] - result.plane.offset;
                if dist.abs() > tol * 4.0 + 1e-12 {
                    return Err(format!(
                        "contour {} point {} is off the plane by {dist}",
                        c.id,
                        format_vec(p)
                    ));
                }
            }
            match (c.signed_area, c.area_square_meters) {
                (Some(a), Some(b)) if a.is_finite() && b.is_finite() => {}
                _ => return Err(format!("closed contour {} lacks finite areas", c.id)),
            }
        } else if c.area_square_meters.is_some() || c.signed_area.is_some() {
            return Err(format!("open chain {} must not carry an area", c.id));
        }
    }

    let outer_ids: std::collections::BTreeSet<&str> = result
        .contours
        .iter()
        .filter(|c| c.closed && c.kind == "outer")
        .map(|c| c.id.as_str())
        .collect();
    for cap in &result.caps {
        if !outer_ids.contains(cap.outer_contour_id.as_str()) {
            return Err(format!(
                "cap {} references unknown outer contour {}",
                cap.id, cap.outer_contour_id
            ));
        }
        if cap.triangles.is_empty() {
            return Err(format!("cap {} has no triangles", cap.id));
        }
        for tri in &cap.triangles {
            for p in tri {
                let dist = n[0] * p[0] + n[1] * p[1] + n[2] * p[2] - result.plane.offset;
                if dist.abs() > tol * 4.0 + 1e-12 {
                    return Err(format!("cap {} vertex off plane by {dist}", cap.id));
                }
            }
        }
        if !(cap.area.is_finite() && cap.area >= 0.0) {
            return Err(format!("cap {} has invalid area {}", cap.id, cap.area));
        }
    }

    if require_closed && closed == 0 {
        return Err("requireClosed set but the document contains no closed contour".into());
    }

    Ok(format!(
        "result_id={} contours={} closed={} caps={} tol={:.3e}",
        result.result_id,
        result.contours.len(),
        closed,
        result.caps.len(),
        tol
    ))
}

// --- release lock ----------------------------------------------------------

fn lock(args: &[String]) -> ExitCode {
    let mut repo_root = PathBuf::from(".");
    let mut out: Option<PathBuf> = None;
    let mut check = false;
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--repo-root" => {
                repo_root = PathBuf::from(&args[idx + 1]);
                idx += 2;
            }
            "--out" => {
                out = Some(PathBuf::from(&args[idx + 1]));
                idx += 2;
            }
            "--check" => {
                check = true;
                idx += 1;
            }
            other => {
                eprintln!("unknown lock flag: {other}");
                return ExitCode::from(2);
            }
        }
    }
    let lock = match build_release_lock(&repo_root) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("release lock build failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let json = match serde_json::to_vec_pretty(&lock) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("release lock serialization failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(out) = out else {
        eprintln!("lock requires --out");
        return ExitCode::from(2);
    };
    if check {
        // --check compares every stable field against the committed lock. The
        // generation timestamp and toolchain string are intentionally excluded:
        // they record where a lock was produced, not what it pins.
        match fs::read_to_string(&out) {
            Ok(existing) => {
                let current: Value = serde_json::from_slice(&json).unwrap();
                let committed: Value = match serde_json::from_str(&existing) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("committed lock {} is invalid: {e}", out.display());
                        return ExitCode::FAILURE;
                    }
                };
                let stable = |v: &Value| {
                    let mut m = v.as_object().unwrap().clone();
                    m.remove("generatedAt");
                    m.remove("rustToolchain");
                    Value::Object(m)
                };
                if stable(&current) != stable(&committed) {
                    eprintln!(
                        "release lock drift: regenerate {} (pinned deps, fixtures or tolerance rules changed)",
                        out.display()
                    );
                    return ExitCode::FAILURE;
                }
                println!("release lock ok: {}", out.display());
                ExitCode::SUCCESS
            }
            Err(_) => {
                eprintln!("--check set but {} does not exist; run the release without --check first", out.display());
                ExitCode::FAILURE
            }
        }
    } else {
        if let Err(e) = write_atomic(&out, &json) {
            eprintln!("could not write lock: {e}");
            return ExitCode::FAILURE;
        }
        println!("wrote release lock {}", out.display());
        ExitCode::SUCCESS
    }
}

// ---------------------------------------------------------------------------
// Release lock (pinned geometry dependencies, tolerance rules, test models)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseLock {
    pub schema: String,
    pub tool: String,
    pub version: String,
    pub generated_at: String,
    pub rust_toolchain: String,
    pub dependencies: BTreeMap<String, String>,
    pub pinned_fixtures: Vec<PinnedFixture>,
    pub tolerance: Value,
    pub acceptance: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinnedFixture {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

/// Builds the checked-in release lock from Cargo.toml plus the fixture files
/// the acceptance runs actually consume. The lock is regenerated by the
/// pipeline on every release and compared against the committed one.
pub fn build_release_lock(repo_root: &Path) -> Result<ReleaseLock, String> {
    let cargo_toml = repo_root.join("server/Cargo.toml");
    let text = fs::read_to_string(&cargo_toml).map_err(|e| e.to_string())?;
    let table: Value = toml_lite_parse(&text)?;
    let pkg = table
        .get("package")
        .ok_or("Cargo.toml missing [package]")?;
    let version = pkg
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("0.0.0")
        .to_string();

    let mut dependencies = BTreeMap::new();
    if let Some(deps) = table.get("dependencies").and_then(Value::as_object) {
        for (name, spec) in deps {
            let pinned = match spec {
                Value::String(v) => v.clone(),
                other => other.to_string(),
            };
            dependencies.insert(name.clone(), pinned);
        }
    }

    let fixtures_dir = repo_root.join("fixtures");
    let mut paths: Vec<PathBuf> = fs::read_dir(&fixtures_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    let mut pinned_fixtures = Vec::new();
    for p in paths {
        let bytes = fs::read(&p).map_err(|e| e.to_string())?;
        let rel = p
            .strip_prefix(repo_root)
            .map(|x| x.display().to_string())
            .unwrap_or_else(|_| p.display().to_string());
        pinned_fixtures.push(PinnedFixture {
            path: rel,
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
        });
    }

    Ok(ReleaseLock {
        schema: "archisection.release-lock/1".into(),
        tool: "archisection".into(),
        version,
        generated_at: now_utc(),
        rust_toolchain: rustc_version_string(),
        dependencies,
        pinned_fixtures,
        tolerance: serde_json::json!({
            "linearRule": "tol = clamp(diagonal * 1e-7, 1e-9, 1e-2) in model units",
            "angular": 1e-9,
            "onPlanePredicate": "abs(dot(n,p) - d) <= tol",
            "shortSegmentRule": "segments shorter than 4*tol are dropped and counted",
            "noNearestEndpointJoins": true
        }),
        acceptance: serde_json::json!({
            "kernelTests": "cargo test --release",
            "edgeCases": ["plane through mesh vertices", "coplanar triangles", "open shell"],
            "verify": "section-server verify --result <file> --require-closed",
            "failureGating": "a non-zero CLI exit blocks promotion into the release directory",
            "uiEntry": "./start.sh then open the printed URL"
        }),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_world(path: &Path) -> Result<WorldScene, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let scene: Scene = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    WorldScene::build(&scene).map_err(|errs| {
        errs.iter()
            .map(|e| format!("[{}] {}", e.code, e.message))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

fn load_job_scene(job: &BatchJob, job_path: &Path, out_dir: &Path) -> Result<WorldScene, String> {
    match (&job.scene, &job.scene_path) {
        (Some(value), _) => {
            let scene: Scene = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            // Persist the inline model so the output directory is reloadable.
            let pretty = serde_json::to_vec_pretty(&scene).map_err(|e| e.to_string())?;
            fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
            write_atomic(&out_dir.join("scene.json"), &pretty).map_err(|e| e.to_string())?;
            WorldScene::build(&scene).map_err(|errs| {
                errs.iter()
                    .map(|e| format!("[{}] {}", e.code, e.message))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
        }
        (None, Some(rel)) => {
            let base = job_path.parent().unwrap_or_else(|| Path::new("."));
            let full = if rel.is_absolute() {
                rel.clone()
            } else {
                base.join(rel)
            };
            load_world(&full)
        }
        (None, None) => Err("job needs either scenePath or an inline scene".into()),
    }
}

/// Writes via a temp file in the same directory and renames into place, so a
/// failed command can never leave a half-written result that looks complete.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|x| x.to_str()).unwrap_or("out")
    ));
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

fn safe_file_name(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if out.is_empty() { "cut".into() } else { out }
}

fn format_vec(p: &[f64; 3]) -> String {
    format!("({},{},{})", p[0], p[1], p[2])
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hash = [0u8; 32];
    sha256(bytes, &mut hash);
    let mut s = String::with_capacity(64);
    for b in hash {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Minimal FIPS 180-4 SHA-256 so the release lock needs no extra crate.
fn sha256(message: &[u8], out: &mut [u8; 32]) {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len = (message.len() as u64).wrapping_mul(8);
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
}

/// Tiny TOML reader for the flat `[package]` / `[dependencies]` tables we own.
/// Avoids adding a TOML crate just to hash our own manifest.
fn toml_lite_parse(text: &str) -> Result<Value, String> {
    let mut root = serde_json::Map::new();
    let mut current = String::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            current = header
                .trim_end_matches(']')
                .trim()
                .trim_matches('"')
                .to_string();
            root.entry(current.clone())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches('"').to_string();
        let value = value.trim().split('#').next().unwrap_or("").trim().trim_matches('"');
        let parsed = if let Ok(n) = value.parse::<i64>() {
            Value::from(n)
        } else if let Ok(b) = value.parse::<bool>() {
            Value::from(b)
        } else {
            Value::String(value.to_string())
        };
        let table = root
            .entry(current.clone())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Value::Object(map) = table {
            map.insert(key, parsed);
        }
    }
    Ok(Value::Object(root))
}

fn rustc_version_string() -> String {
    // Try PATH first, then the standard rustup location used by the pipeline.
    let candidates = ["rustc".to_string(), format!("{}/.cargo/bin/rustc", std::env::var("HOME").unwrap_or_default())];
    for prog in candidates {
        if let Ok(out) = std::process::Command::new(&prog).arg("--version").output() {
            if out.status.success() {
                return String::from_utf8_lossy(&out.stdout).trim().to_string();
            }
        }
    }
    "rustc (version unavailable)".into()
}

fn now_utc() -> String {
    // Seconds since epoch is dependency-free and sufficient for a lock stamp.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}
