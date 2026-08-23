use axum::{
    body::Body,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::Response,
};
use std::path::{Component, Path, PathBuf};

#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;

const INDEX_FILE: &str = "index.html";
const INDEX_BASE_HREF_PLACEHOLDER: &str = "JUGGLINGLAB_BASE_HREF";
#[cfg(not(debug_assertions))]
const EMBEDDED_INDEX: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../index.html"));

#[derive(Clone)]
pub(crate) struct FrontendAssets {
    #[cfg(debug_assertions)]
    root: PathBuf,
    base_path: String,
    base_href: String,
}

#[cfg(not(debug_assertions))]
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../dist/"]
struct EmbeddedDist;

impl FrontendAssets {
    #[cfg(debug_assertions)]
    pub(crate) fn new(root: PathBuf, base_path: String) -> Self {
        Self {
            root,
            base_href: base_href(&base_path),
            base_path,
        }
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn new(_root: PathBuf, base_path: String) -> Self {
        Self {
            base_href: base_href(&base_path),
            base_path,
        }
    }

    fn request_path<'a>(&self, path: &'a str) -> Option<&'a str> {
        if self.base_path == "/" {
            return path.strip_prefix('/');
        }
        if path == self.base_path {
            return Some("");
        }
        path.strip_prefix(&self.base_path)?.strip_prefix('/')
    }

    #[cfg(debug_assertions)]
    fn asset(&self, path: &str) -> Result<Option<Vec<u8>>, std::io::Error> {
        let file_path = self.root.join(path);
        match std::fs::read(file_path) {
            Ok(data) => Ok(Some(data)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    #[cfg(not(debug_assertions))]
    fn asset(&self, path: &str) -> Result<Option<Vec<u8>>, std::io::Error> {
        Ok(EmbeddedDist::get(path).map(|file| file.data.into_owned()))
    }

    fn index(&self) -> Result<Vec<u8>, std::io::Error> {
        #[cfg(debug_assertions)]
        let source = match self.asset(INDEX_FILE)? {
            Some(source) => source,
            None => {
                std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../index.html"))?
            }
        };
        #[cfg(not(debug_assertions))]
        let source = EMBEDDED_INDEX.to_vec();

        Ok(String::from_utf8_lossy(&source)
            .replace(INDEX_BASE_HREF_PLACEHOLDER, &self.base_href)
            .into_bytes())
    }
}

pub(crate) fn base_href(base_path: &str) -> String {
    if base_path == "/" {
        "/".to_string()
    } else {
        format!("{base_path}/")
    }
}

pub(crate) async fn fallback(State(assets): State<FrontendAssets>, request: Request) -> Response {
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return response(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"404 Not Found".to_vec(),
            false,
        );
    }

    let Some(request_path) = assets.request_path(request.uri().path()) else {
        return not_found();
    };
    if request_path.is_empty() {
        return index_response(&assets, request.method() == Method::HEAD);
    }
    let Some(path) = safe_relative_path(request_path) else {
        return not_found();
    };
    if path == INDEX_FILE {
        return index_response(&assets, request.method() == Method::HEAD);
    }

    match assets.asset(path) {
        Ok(Some(data)) => response(
            StatusCode::OK,
            mime_guess::from_path(path).first_or_octet_stream().as_ref(),
            data,
            request.method() == Method::HEAD,
        ),
        Ok(None) if !looks_like_asset(path) => {
            index_response(&assets, request.method() == Method::HEAD)
        }
        Ok(None) => not_found(),
        Err(error) => response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "text/plain; charset=utf-8",
            format!("Unable to read frontend asset: {error}").into_bytes(),
            request.method() == Method::HEAD,
        ),
    }
}

fn index_response(assets: &FrontendAssets, head: bool) -> Response {
    match assets.index() {
        Ok(data) => response(StatusCode::OK, "text/html; charset=utf-8", data, head),
        Err(error) => response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "text/plain; charset=utf-8",
            format!("Unable to read index.html: {error}").into_bytes(),
            head,
        ),
    }
}

fn response(status: StatusCode, content_type: &str, data: Vec<u8>, head: bool) -> Response {
    let length = data.len();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, length)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(if head {
            Body::empty()
        } else {
            Body::from(data)
        })
        .expect("valid static asset response")
}

fn not_found() -> Response {
    response(
        StatusCode::NOT_FOUND,
        "text/plain; charset=utf-8",
        b"404 Not Found".to_vec(),
        false,
    )
}

fn safe_relative_path(path: &str) -> Option<&str> {
    (!path.is_empty()
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_))))
    .then_some(path)
}

fn looks_like_asset(path: &str) -> bool {
    path.starts_with("api/")
        || path.starts_with("pkg/")
        || path.starts_with("assets/")
        || Path::new(path).extension().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_paths_only_inside_the_configured_base_path() {
        let assets = FrontendAssets::new(PathBuf::from("dist"), "/jugglinglab".to_string());
        assert_eq!(assets.request_path("/jugglinglab"), Some(""));
        assert_eq!(assets.request_path("/jugglinglab/"), Some(""));
        assert_eq!(
            assets.request_path("/jugglinglab/pkg/juggling_web.js"),
            Some("pkg/juggling_web.js")
        );
        assert_eq!(assets.request_path("/pkg/juggling_web.js"), None);
    }

    #[test]
    fn rejects_paths_that_can_escape_dist() {
        assert!(safe_relative_path("pkg/juggling_web.js").is_some());
        assert!(safe_relative_path("../Cargo.toml").is_none());
        assert!(safe_relative_path("assets\\..\\Cargo.toml").is_none());
        assert!(safe_relative_path("").is_none());
    }

    #[test]
    fn base_href_always_ends_with_a_slash() {
        assert_eq!(base_href("/"), "/");
        assert_eq!(base_href("/jugglinglab"), "/jugglinglab/");
    }
}
