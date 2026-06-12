use axum::{
    body::Body,
    http::{header, Response, StatusCode, Uri},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../app/dist/"]
struct Assets;

pub async fn static_handler(uri: Uri) -> Response<Body> {
    let path = uri.path().trim_start_matches('/');
    let asset_path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(asset_path).or_else(|| Assets::get("index.html")) {
        Some(content) => {
            let mime = mime_guess::from_path(asset_path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data.into_owned()))
                .expect("static response")
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("front assets not built"))
            .expect("not found response"),
    }
}

