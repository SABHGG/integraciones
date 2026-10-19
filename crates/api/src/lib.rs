//! Servidor API: plano de administración `/api/v1` (Bearer, Problem Details)
//! y plano de invocación `/x` (sin auth en v1).
//!
//! El binario (`integraciones-api`) solo cablea el pool, las migraciones y el
//! listener; el enrutado y los handlers viven aquí para que los tests de
//! integración puedan montar la misma aplicación con Postgres real.

pub mod auth;
pub mod blocks;
pub mod bridges;
pub mod credentials;
pub mod cursor;
pub mod docs;
pub mod executions;
pub mod invoke;
pub mod problem;
pub mod rest;
pub mod retention;
pub mod state;
pub mod transpile;
pub mod ui;
pub mod vault;
pub mod webhooks;
pub mod worker;

use axum::Json;
use axum::Router;
use axum::extract::Extension;
use axum::middleware;
use axum::routing::get;
use serde_json::json;
use sqlx::PgPool;

use state::AppState;

/// Abre el pool de conexiones con la configuración de producción.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}

/// Aplica las migraciones embebidas (`crates/api/migrations`).
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

/// Construye la aplicación axum completa: los dos planos del design.md.
pub fn app(state: AppState) -> Router {
    // Plano de administración: Bearer + Problem Details (RFC 9457)
    let admin = Router::new()
        .route("/whoami", get(whoami))
        .route("/healthz", get(|| async { "ok" }))
        .merge(blocks::router())
        .merge(credentials::router())
        .merge(bridges::router())
        .merge(executions::router())
        // Cualquier ruta /api/v1 desconocida responde Problem Details: el
        // cliente del plano admin solo habla application/problem+json.
        .fallback(|| async { problem::not_found("la ruta no existe en la API") })
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_api_key,
        ));

    // Plano de invocación: sin auth de administración en v1 (spec
    // `bridge-invocation`); la auth por puente queda para una versión futura.
    let invocation = invoke::router()
        .fallback(|| async { problem::not_found("las invocaciones usan /x/{slug} del puente") });

    // Documentación embebida: OpenAPI 3.1 + UI de Scalar (sin auth: leer la
    // referencia no revela datos, y la UI vive junto a la API que documenta).
    let docs = docs::doc_router();

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(docs)
        .nest("/api/v1", admin)
        .nest("/x", invocation)
        .with_state(state)
}

/// Identidad autenticada (útil para diagnosticar la key y su workspace).
async fn whoami(Extension(key): Extension<auth::AuthedKey>) -> Json<serde_json::Value> {
    Json(json!({
        "id": key.id,
        "name": key.name,
        "workspaceId": key.workspace_id,
    }))
}
