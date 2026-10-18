//! Documentación de la API embebida en el propio servidor.
//!
//! Dos rutas estáticas, sin base de datos:
//!
//! - `/openapi.json`: el documento OpenAPI 3.1 (`crates/api/openapi.json`)
//!   incluido en el binario — una sola fuente de verdad versionada con el código.
//! - `/docs`: la UI de Scalar que consume el documento embebido, montada con
//!   el crate oficial `scalar_api_reference` (HTML + bundle JS embebidos, sin
//!   depender de ningún CDN).

use axum::Json;
use axum::Router;
use axum::routing::get;
use serde_json::Value;

use crate::state::AppState;

/// Documento OpenAPI 3.1 embebido en el binario.
const OPENAPI_JSON: &str = include_str!("../openapi.json");

/// Configuración de la UI de Scalar: el documento y el título de la API.
const SCALAR_CONFIG: &str = r#"{"url": "/openapi.json", "title": "Integraciones API"}"#;

/// Configuración parseada una única vez (es una constante de compilación).
fn scalar_config() -> Value {
    serde_json::from_str(SCALAR_CONFIG).expect("la configuración de Scalar es JSON válido")
}

/// Router de documentación: el documento OpenAPI + la referencia de Scalar.
///
/// El router de Scalar nace sin estado (`Router<()>`): sus handlers no leen
/// `State`, así que `with_state(())` lo re-tipea al `AppState` del crate para
/// poder fusionarlo con el resto de la aplicación.
pub fn doc_router() -> Router<AppState> {
    Router::new()
        .route("/openapi.json", get(documento_openapi))
        .merge(scalar_api_reference::axum::router("/docs", &scalar_config()).with_state(()))
}

/// Sirve el documento OpenAPI embebido.
async fn documento_openapi() -> Json<Value> {
    Json(serde_json::from_str(OPENAPI_JSON).expect("openapi.json embebido es JSON válido"))
}
