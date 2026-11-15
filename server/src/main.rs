use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use section_kernel::cli;
use section_kernel::model::WorldScene;
use section_kernel::section::{build_section, SectionRequest};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SectionPayload {
    #[serde(default)]
    scene: Option<serde_json::Value>,
    /// ID of a previously loaded scene (keeps the browser payload small).
    #[serde(default)]
    scene_id: Option<String>,
    #[serde(flatten)]
    request: SectionRequest,
}

#[derive(Clone, Default)]
struct Store {
    /// Scene definitions kept server-side so a live drag only sends the plane.
    scenes: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<WorldScene>>>>,
}

fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> axum::response::Response {
    (
        status,
        Json(json!({
            "ok": false,
            "error": {
                "code": code,
                "message": message.into(),
            }
        })),
    )
        .into_response()
}

fn scene_errors(errs: Vec<section_kernel::SceneError>) -> axum::response::Response {
    let status = if errs.iter().any(|e| {
        matches!(
            e.code,
            "invalid_plane" | "invalid_unit" | "missing_object_id"
        )
    }) {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::UNPROCESSABLE_ENTITY
    };
    (
        status,
        Json(json!({
            "ok": false,
            "error": {
                "code": "scene_invalid",
                "message": "the scene could not be sectioned",
                "details": errs.iter().map(|e| json!({
                    "code": e.code,
                    "message": e.message,
                    "objectId": e.object_id,
                })).collect::<Vec<_>>(),
            }
        })),
    )
        .into_response()
}

async fn health() -> impl IntoResponse {
    Json(json!({
        "ok": true,
        "service": "archisection",
        "kernel": "section_kernel",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn load_scene(
    axum::extract::State(store): axum::extract::State<Store>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let scene: section_kernel::Scene = match serde_json::from_value(body.clone()) {
        Ok(s) => s,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, "invalid_json", format!("scene could not be parsed: {e}")),
    };
    match WorldScene::build(&scene) {
        Ok(world) => {
            let id = format!("scene-{:016x}", fnv1a64(&body.to_string()));
            let summary = json!({
                "ok": true,
                "sceneId": id,
                "name": world.name,
                "unit": {
                    "name": world.unit_name,
                    "metersPerUnit": world.meters_per_unit,
                },
                "objects": world.objects.iter().map(|o| json!({
                    "id": o.id,
                    "name": o.name,
                    "group": o.group,
                    "triangles": o.triangles.len(),
                    "mirrored": o.mirrored,
                })).collect::<Vec<_>>(),
                "bounds": {
                    "min": [world.aabb.min.x, world.aabb.min.y, world.aabb.min.z],
                    "max": [world.aabb.max.x, world.aabb.max.y, world.aabb.max.z],
                    "diagonal": world.aabb.diagonal(),
                },
                "bakeWarnings": world.bake_warnings,
            });
            store.scenes.lock().unwrap().insert(id.clone(), Arc::new(world));
            (StatusCode::OK, Json(summary)).into_response()
        }
        Err(errs) => scene_errors(errs),
    }
}

async fn section(
    axum::extract::State(store): axum::extract::State<Store>,
    Json(payload): Json<SectionPayload>,
) -> impl IntoResponse {
    let world = match (&payload.scene, &payload.scene_id) {
        (Some(value), _) => {
            let scene: section_kernel::Scene = match serde_json::from_value(value.clone()) {
                Ok(s) => s,
                Err(e) => {
                    return api_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_json",
                        format!("scene could not be parsed: {e}"),
                    )
                }
            };
            match WorldScene::build(&scene) {
                Ok(w) => Arc::new(w),
                Err(errs) => return scene_errors(errs),
            }
        }
        (None, Some(id)) => match store.scenes.lock().unwrap().get(id) {
            Some(w) => w.clone(),
            None => {
                return api_error(
                    StatusCode::NOT_FOUND,
                    "unknown_scene",
                    format!("scene '{id}' is not loaded; POST it to /api/scene first"),
                )
            }
        },
        (None, None) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "missing_scene",
                "send either a scene document or a sceneId",
            )
        }
    };
    match build_section(&payload.request, &world) {
        Ok(result) => {
            let body = json!({ "ok": true, "result": result });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(errs) => scene_errors(errs),
    }
}

fn fnv1a64(bytes: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// HTTP/UI server. Invoked by the `serve` sub-command and by the legacy
/// no-argument entry point so `start.sh` keeps working unchanged.
pub fn serve_http(args: &[String]) -> ExitCode {
    let mut port: Option<u16> = None;
    let mut static_dir: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        let value = || args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--port" => {
                port = value().parse().ok();
                i += 2;
            }
            "--static-dir" => {
                static_dir = Some(PathBuf::from(value()));
                i += 2;
            }
            other => {
                eprintln!("unknown serve flag: {other}");
                return ExitCode::from(2);
            }
        }
    }
    let port = port
        .or_else(|| std::env::var("PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(8787);
    let static_dir = static_dir
        .or_else(|| std::env::var("STATIC_DIR").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("web/dist"));

    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("failed to start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    rt.block_on(async move {
        let store = Store::default();
        let cors = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST])
            .allow_origin(HeaderValue::from_static("*"));

        let mut app = Router::new()
            .route("/api/health", get(health))
            .route("/api/scene", post(load_scene))
            .route("/api/section", post(section))
            .layer(DefaultBodyLimit::max(128 * 1024 * 1024))
            .layer(cors)
            .with_state(store);

        if static_dir.is_dir() {
            // Serve the built UI and fall back to index.html for the SPA.
            let index = static_dir.join("index.html");
            app = app.fallback_service(
                ServeDir::new(&static_dir).not_found_service(ServeFile::new(index)),
            );
        }

        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        println!("archisection kernel listening on http://{addr}");
        if static_dir.is_dir() {
            println!("serving UI from {}", static_dir.display());
        } else {
            println!(
                "UI directory {} not found; API is available at /api (run `npm run build` in web/)",
                static_dir.display()
            );
        }
        let listener = tokio::net::TcpListener::bind(addr).await.expect("bind port");
        axum::serve(listener, app).await.expect("server");
    });
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    section_kernel::cli::install_serve(serve_http);
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() {
        // Backwards-compatible default: the launcher script historically ran
        // the binary with no arguments to get the combined API + UI server.
        return serve_http(&argv);
    }
    cli::run(&argv)
}
