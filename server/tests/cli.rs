//! End-to-end tests for the headless CLI. These spawn the actual built
//! `section-server` binary against the real fixtures, so they exercise the
//! same path the release pipeline and background batch jobs use — including
//! process exit codes, atomic output files and the failure gate.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target/release/section-server");
    assert!(p.exists(), "run `cargo build --release` before CLI tests");
    p
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "archisection-cli-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn single_section_writes_a_reloadable_result() {
    let tmp = Temp(temp_dir("single"));
    let scene = repo_root().join("fixtures/demo-building.json");
    let out = tmp.0.join("plan.json");
    let status = Command::new(bin())
        .arg("section")
        .arg("--scene")
        .arg(&scene)
        .arg("--normal")
        .arg("0,0,1")
        .arg("--offset")
        .arg("1.5")
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(out.exists());

    let verify = Command::new(bin())
        .arg("verify")
        .arg("--result")
        .arg(&out)
        .arg("--require-closed")
        .status()
        .unwrap();
    assert!(verify.success(), "produced result must reload and verify");
}

#[test]
fn batch_edge_cases_succeeds_and_every_result_has_a_matching_manifest() {
    let tmp = Temp(temp_dir("edge"));
    let job = repo_root().join("release/jobs/edge-cases.batch.json");
    let status = Command::new(bin())
        .arg("batch")
        .arg("--job")
        .arg(&job)
        .arg("--out-dir")
        .arg(&tmp.0)
        .status()
        .unwrap();
    assert!(status.success(), "through-vertex and coplanar cuts must pass");

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(tmp.0.join("batch-report.json")).unwrap()).unwrap();
    assert_eq!(report["resultCount"], 2);
    assert!(report["failures"].as_array().unwrap().is_empty());

    for entry in report["results"].as_array().unwrap() {
        let rel = entry["resultPathRel"].as_str().unwrap();
        let result_path = tmp.0.join(rel);
        let result: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&result_path).unwrap()).unwrap();
        assert!(result["stats"]["closedContours"].as_u64().unwrap() >= 1);

        let manifest_path = result_path.with_extension("manifest.json");
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
        let bytes = fs::read(&result_path).unwrap();
        assert_eq!(manifest["resultBytes"].as_u64().unwrap() as usize, bytes.len());
        assert_eq!(manifest["resultId"], result["resultId"]);
        assert_eq!(manifest["resultSha256"], sha256_hex(&bytes));
    }
}

#[test]
fn failed_cut_exits_nonzero_and_leaves_no_result_artifact() {
    let tmp = Temp(temp_dir("failgate"));
    let job = repo_root().join("release/jobs/open-shell-failgate.batch.json");
    let status = Command::new(bin())
        .arg("batch")
        .arg("--job")
        .arg(&job)
        .arg("--out-dir")
        .arg(&tmp.0)
        .status()
        .unwrap();
    assert!(!status.success(), "requireClosed must reject the open shell");
    // Only the report exists; the failed cut never wrote a consumable result.
    let mut files: Vec<String> = Vec::new();
    collect_json(&tmp.0, &mut files);
    assert_eq!(files, vec!["batch-report.json".to_string()], "no result leaked: {files:?}");
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(tmp.0.join("batch-report.json")).unwrap()).unwrap();
    assert_eq!(report["failures"].as_array().unwrap().len(), 1);
}

#[test]
fn verify_rejects_a_document_that_is_not_a_result() {
    let tmp = Temp(temp_dir("badverify"));
    let bogus = tmp.0.join("not-a-result.json");
    fs::write(&bogus, r#"{"ok":false,"error":{"code":"invalid_plane"}}"#).unwrap();
    let status = Command::new(bin())
        .arg("verify")
        .arg("--result")
        .arg(&bogus)
        .status()
        .unwrap();
    assert!(!status.success());
}

#[test]
fn release_lock_is_stable_and_check_mode_detects_drift() {
    let tmp = Temp(temp_dir("lock"));
    let lock = tmp.0.join("release-lock.json");
    let first = Command::new(bin())
        .arg("lock")
        .arg("--repo-root")
        .arg(repo_root())
        .arg("--out")
        .arg(&lock)
        .status()
        .unwrap();
    assert!(first.success());

    // Re-generating into the same committed-style file must pass --check.
    let check = Command::new(bin())
        .arg("lock")
        .arg("--repo-root")
        .arg(repo_root())
        .arg("--out")
        .arg(&lock)
        .arg("--check")
        .status()
        .unwrap();
    assert!(check.success());

    // Tamper with a pinned fixture hash; --check must now fail.
    let mut doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(&lock).unwrap()).unwrap();
    doc["pinnedFixtures"][0]["sha256"] = serde_json::json!("0000000000000000000000000000000000000000000000000000000000000000");
    fs::write(&lock, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    let drifted = Command::new(bin())
        .arg("lock")
        .arg("--repo-root")
        .arg(repo_root())
        .arg("--out")
        .arg(&lock)
        .arg("--check")
        .status()
        .unwrap();
    assert!(!drifted.success(), "lock drift must block the release");
}

fn collect_json(dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_json(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path.strip_prefix(dir).unwrap().display().to_string());
        }
    }
    out.sort();
}

fn sha256_hex(bytes: &[u8]) -> String {
    // Mirror the kernel's FIPS SHA-256 via a compact independent implementation.
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut msg = bytes.to_vec();
    let bit_len = (bytes.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
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
        for i in 0..8 {
            h[i] = h[i].wrapping_add([a, b, c, d, e, f, g, hh][i]);
        }
    }
    h.iter().map(|w| format!("{w:08x}")).collect()
}
