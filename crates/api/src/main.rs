//! Binario del servidor API: CLI auxiliar, tracing, pool, migraciones y
//! listener. El enrutado vive en la librería del crate (`api::app`).

use api::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // CLI auxiliar: `integraciones-api hash-key <key>` imprime el hash para
    // insertarlo en api_keys.key_hash (el crudo nunca se guarda).
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("hash-key") {
        let key = args.get(2).ok_or("uso: hash-key <key>")?;
        println!("{}", api::auth::hash_key(key));
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

    let pool = api::connect(&database_url).await?;
    api::migrate(&pool).await?;
    tracing::info!(migraciones = "ok", "esquema verificado");

    let app = api::app(AppState { pool });

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!(port, "servidor escuchando");
    axum::serve(listener, app).await?;
    Ok(())
}