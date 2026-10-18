//! Tests de la documentación de la API: OpenAPI 3.1 embebido y Scalar API
//! Reference montados por el propio servidor (task de documentación).
//!
//! No necesitan base de datos: son respuestas estáticas del router.

use axum::Router;
use axum::http::{StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;

fn app() -> Router {
    api::app(api::state::AppState {
        pool: sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://docs-test@127.0.0.1:5/docs-test")
            .expect("pool perezoso (no se usa en estas respuestas)"),
    })
}

async fn get(uri: &str) -> (StatusCode, String, String) {
    let response = app()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(uri)
                .body(axum::body::Body::empty())
                .expect("petición válida"),
        )
        .await
        .expect("respuesta");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("cuerpo legible");
    (
        status,
        content_type,
        String::from_utf8(bytes.to_vec()).expect("UTF-8"),
    )
}

#[tokio::test]
async fn sirve_el_documento_openapi_embebido() {
    let (status, content_type, body) = get("/openapi.json").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        content_type.starts_with("application/json"),
        "content-type: {content_type}"
    );

    let doc: Value = serde_json::from_str(&body).expect("el documento es JSON válido");
    assert!(
        doc["openapi"]
            .as_str()
            .unwrap_or_default()
            .starts_with("3.1"),
        "OpenAPI 3.1.x: {}",
        doc["openapi"]
    );
    assert!(doc["info"]["title"].is_string());

    // Los dos planos quedan documentados.
    for ruta in [
        "/whoami",
        "/api/v1/blocks",
        "/api/v1/blocks/{id}",
        "/api/v1/blocks/{id}/versions",
        "/api/v1/credentials",
        "/api/v1/credentials/{id}",
        "/api/v1/bridges",
        "/api/v1/bridges/{id}",
        "/api/v1/bridges/{id}/validate",
        "/api/v1/bridges/{id}/versions",
        "/api/v1/executions",
        "/api/v1/executions/{id}",
        "/api/v1/executions/{id}/trace",
        "/x/{slug}",
    ] {
        assert!(
            doc["paths"].get(ruta).is_some(),
            "falta la ruta {ruta} en el documento"
        );
    }

    // Errores Problem Details y sobre de invocación como schemas reutilizables.
    for schema in ["Problem", "InvokeEnvelope"] {
        assert!(
            doc["components"]["schemas"].get(schema).is_some(),
            "falta el schema {schema}"
        );
    }
}

#[tokio::test]
async fn sirve_la_referencia_de_scalar() {
    let (status, content_type, body) = get("/docs").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        content_type.starts_with("text/html"),
        "content-type: {content_type}"
    );
    assert!(
        body.contains("scalar") || body.contains("Scalar"),
        "la página carga la referencia de Scalar"
    );

    // La UI apunta al documento embebido.
    assert!(
        body.contains("/openapi.json"),
        "la configuración apunta a /openapi.json"
    );
}
