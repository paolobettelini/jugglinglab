use axum::{Json, Router, http::StatusCode, routing::post};
use clap::Parser;
use juggling_core::generator::{GenerationResult, GeneratorLimits, generate_siteswaps};
use juggling_core::transitioner::transition_siteswaps;
use serde::Deserialize;
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

mod assets;

#[derive(Debug, Parser)]
#[command(about = "Serve the JugglingLab web application.")]
struct Args {
    #[arg(
        long,
        env = "JUGGLINGLAB_ADDRESS",
        default_value = "0.0.0.0",
        value_name = "IP"
    )]
    address: IpAddr,

    #[arg(long, env = "PORT", default_value_t = 8080)]
    port: u16,

    #[arg(
        long,
        env = "JUGGLINGLAB_BASE_PATH",
        default_value = "/",
        value_name = "PATH",
        value_parser = normalize_base_path
    )]
    base_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let addr = SocketAddr::new(args.address, args.port);
    let site_root = absolute_site_root(
        std::env::var_os("LEPTOS_SITE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("dist")),
    );
    let frontend_assets = assets::FrontendAssets::new(site_root, args.base_path.clone());

    let generate_route = prefixed_route(&args.base_path, "/api/generate");
    let transition_route = prefixed_route(&args.base_path, "/api/transition");
    let app = Router::new()
        .route(&generate_route, post(generate))
        .route(&transition_route, post(transition))
        .fallback(assets::fallback)
        .with_state(frontend_assets.clone());

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!(
        "JugglingLab listening on http://{addr}{}",
        assets::base_href(&args.base_path),
    );
    axum::serve(listener, app).await?;
    Ok(())
}

fn absolute_site_root(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn prefixed_route(base_path: &str, route: &str) -> String {
    if base_path == "/" {
        format!("/{}", route.trim_start_matches('/'))
    } else {
        format!("{base_path}/{}", route.trim_start_matches('/'))
    }
}

fn normalize_base_path(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("base-path cannot be empty".to_string());
    }
    if !value.starts_with('/') {
        return Err("base-path must start with /".to_string());
    }
    if value.contains(['?', '#', '\\', '<', '>', '"', '\'']) || value.chars().any(char::is_control)
    {
        return Err("base-path must be a plain URL path without query or fragment".to_string());
    }
    if value == "/" {
        return Ok("/".to_string());
    }

    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty()
        || trimmed
            .split('/')
            .skip(1)
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err("base-path cannot contain empty, . or .. segments".to_string());
    }
    Ok(trimmed.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_paths_are_normalized_for_router_scopes() {
        assert_eq!(normalize_base_path("/").unwrap(), "/");
        assert_eq!(
            normalize_base_path("/jugglinglab/").unwrap(),
            "/jugglinglab"
        );
        assert_eq!(
            prefixed_route("/jugglinglab", "/api/generate"),
            "/jugglinglab/api/generate"
        );
        assert_eq!(prefixed_route("/", "/api/generate"), "/api/generate");
    }

    #[test]
    fn invalid_base_paths_are_rejected() {
        for invalid in ["", "jugglinglab", "//jugglinglab", "/../private", "/a//b"] {
            assert!(normalize_base_path(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn deployment_path_is_exposed_only_as_base_path() {
        let args = Args::try_parse_from([
            "jugglinglab-web",
            "--address",
            "127.0.0.1",
            "--port",
            "9000",
            "--base-path",
            "/jugglinglab/",
        ])
        .unwrap();
        assert_eq!(args.address, "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(args.port, 9000);
        assert_eq!(args.base_path, "/jugglinglab");

        assert!(Args::try_parse_from(["jugglinglab-web", "--site-root", "/jugglinglab"]).is_err());
    }
}
