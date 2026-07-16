use axum::{Json, Router, http::StatusCode, routing::post};
use juggling_core::generator::{GenerationResult, GeneratorLimits, generate_siteswaps};
use juggling_core::transitioner::transition_siteswaps;
use serde::Deserialize;
use std::{
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let public_dir = env::var("JUGGLINGLAB_PUBLIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let working_dir_public = env::current_dir().unwrap_or_default().join("public");
            if working_dir_public.is_dir() {
                working_dir_public
            } else {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../public")
            }
        });
    let public_dir = public_dir.canonicalize().unwrap_or(public_dir);

    let app = Router::new()
        .route("/api/generate", post(generate))
        .route("/api/transition", post(transition))
        .fallback_service(
            ServeDir::new(&public_dir)
                .not_found_service(ServeFile::new(public_dir.join("index.html"))),
        );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!(
        "JugglingLab web server listening on http://{addr} serving {}",
        public_dir.display()
    );
    axum::serve(listener, app).await?;
    Ok(())
}

async fn transition(
    Json(request): Json<GenerateRequest>,
) -> Result<Json<GenerationResult>, (StatusCode, String)> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let mut guard = CancellationGuard::new(cancelled);
    let result = tokio::task::spawn_blocking(move || {
        transition_siteswaps(
            &request.arguments,
            GeneratorLimits {
                max_patterns: Some(1_000),
                max_time: Some(std::time::Duration::from_secs(15)),
                cancelled: Some(worker_cancelled),
            },
        )
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Transitioner worker failed: {error}"),
        )
    })?
    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    guard.disarm();
    Ok(Json(result))
}

#[derive(Deserialize)]
struct GenerateRequest {
    arguments: String,
}

async fn generate(
    Json(request): Json<GenerateRequest>,
) -> Result<Json<GenerationResult>, (StatusCode, String)> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let mut guard = CancellationGuard::new(cancelled);
    let result = tokio::task::spawn_blocking(move || {
        generate_siteswaps(
            &request.arguments,
            GeneratorLimits {
                max_patterns: Some(1_000),
                max_time: Some(std::time::Duration::from_secs(15)),
                cancelled: Some(worker_cancelled),
            },
        )
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Generator worker failed: {error}"),
        )
    })?
    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    guard.disarm();
    Ok(Json(result))
}

struct CancellationGuard {
    cancelled: Arc<AtomicBool>,
    armed: bool,
}

impl CancellationGuard {
    fn new(cancelled: Arc<AtomicBool>) -> Self {
        Self {
            cancelled,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cancelled.store(true, Ordering::Relaxed);
        }
    }
}
