use axum::Router;
use std::{env, net::SocketAddr, path::PathBuf};
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
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../public"));
    let public_dir = public_dir.canonicalize().unwrap_or(public_dir);

    let app = Router::new().fallback_service(
        ServeDir::new(&public_dir).not_found_service(ServeFile::new(public_dir.join("index.html"))),
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!(
        "JugglingLab web server listening on http://{addr} serving {}",
        public_dir.display()
    );
    axum::serve(listener, app).await?;
    Ok(())
}
