//! Servidor API: plano de administración `/api/v1` (Bearer, Problem Details)
//! y plano de invocación `/x` (sin auth en v1). Migraciones al arranque.

mod auth;
mod problem;
mod state;

use axum::extract::Extension;
use axum::middleware;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // CLI auxiliar: `integraciones-api hash-key <key>` imprime el hash para
    // insertarlo en api_keys.key_hash (el crudo nunca se guarda).
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("hash-key") {
        let key = args.get(2).ok_or("uso: hash-key <key>")?;
        println!("{}", auth::hash_key(key));
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL no definida (ver README.md)")?;
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!(migraciones = "ok", "esquema verificado");

    let state = AppState { pool };

    // Plano de administración: Bearer + Problem Details (RFC 9457)
    let admin = Router::new()
        .route("/whoami", get(whoami))
        .route("/healthz", get(|| async { "ok" }))
        .route("/blocks", post(|| async {
            problem::not_implemented("El CRUD de bloques llega en la Fase 4 (tarea 4.1)")
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_api_key,
        ));

    // Plano de invocación: sin auth en v1; las rutas /x/{slug} llegan en 4.4
    let invocation = Router::new().route(
        "/",
        get(|| async {
            problem::not_found("Las invocaciones usan /x/{slug} del puente")
        }),
    );

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/api/v1", admin)
        .nest("/x", invocation)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!(port, "servidor escuchando");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn whoami(Extension(key): Extension<auth::AuthedKey>) -> Json<serde_json::Value> {
    Json(json!({ "id": key.id, "name": key.name }))
}
