//! Consola web embebida (MVP3, spec `web-console`): assets estáticos de
//! `console/dist/` servidos por el mismo binario bajo `/console`.
//!
//! Cero backend nuevo: la SPA consume exclusivamente el plano admin
//! existente con el Bearer de la API key que introduce en el login. El
//! embedding usa `rust-embed` (lee del disco en debug, embebe en release);
//! `build.rs` genera un stub si `console/dist/` no existe para que compilar
//! sin Node no rompa.

use axum::Router;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use rust_embed::RustEmbed;

/// Assets de la consola: `console/dist/` generado por Vite.
#[derive(RustEmbed)]
#[folder = "../../console/dist/"]
struct Assets;

fn respuesta(ruta: &str) -> Option<Response> {
    let archivo = Assets::get(ruta)?;
    let mime = mime_guess::from_path(ruta).first_or_octet_stream();
    Some(
        (
            [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
            archivo.data,
        )
            .into_response(),
    )
}

/// El `index.html` del dist (o el aviso si el stub no trae nada útil).
fn index_respuesta() -> Response {
    respuesta("index.html")
        .unwrap_or_else(|| Html("<h1>consola no construida</h1>".to_string()).into_response())
}

/// `GET /console`: la SPA.
async fn index() -> impl IntoResponse {
    index_respuesta()
}

/// `GET /console/{ruta}`: assets por content-type; cualquier otra ruta cae a
/// `index.html` (SPA con History API).
async fn recurso(axum::extract::Path(ruta): axum::extract::Path<String>) -> Response {
    // Solo servimos archivos reales del dist: sin subida de directorio.
    if ruta.contains("..") {
        return index_respuesta();
    }
    respuesta(&ruta).unwrap_or_else(index_respuesta)
}

/// Router de la consola, montado en la raíz (junto a `/openapi.json`).
pub fn router() -> Router<crate::state::AppState> {
    Router::new()
        .route("/console", get(index))
        .route("/console/", get(index))
        .route("/console/{*ruta}", get(recurso))
}
