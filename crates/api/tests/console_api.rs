//! Tests de serving de la consola embebida (MVP3, task 5.1).
//!
//! La consola no añade endpoints de negocio: el flujo completo ya está
//! cubierto por los tests del plano admin. Aquí se verifica que el binario
//! sirve los assets bajo `/console` con el fallback de SPA.
//!
//! Si `DATABASE_URL` no está definida (CI sin base de datos) los tests se
//! omiten. En local:
//!
//! ```text
//! ./scripts/dev-postgres.sh && (cd console && npm install && npm run build)
//! DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones cargo test -p api --test console_api
//! ```

use api::auth::hash_key;
use api::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

/// Los tests comparten base de datos: se serializan.
static DB_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

struct Harness {
    app: Router,
    pool: PgPool,
    /// Turno de base de datos, vivo mientras dura el test.
    _turno: MutexGuard<'static, ()>,
}

async fn begin() -> Option<Harness> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!(
                "DATABASE_URL no está definida: se omiten los tests de integración de la API"
            );
            return None;
        }
    };

    let pool = api::connect(&url)
        .await
        .expect("conexión a Postgres (¿corriste ./scripts/dev-postgres.sh?)");
    api::migrate(&pool).await.expect("migraciones aplicadas");

    let turno = DB_LOCK.lock().await;
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'console-test%'")
        .execute(&pool)
        .await
        .expect("limpieza de keys previas");

    let workspace_id = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace de test")
        .bind(format!("console-test-{workspace_id}"))
        .execute(&pool)
        .await
        .expect("workspace");
    sqlx::query("INSERT INTO api_keys (id, workspace_id, name, key_hash) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::now_v7())
        .bind(workspace_id)
        .bind("console-test-key")
        .bind(hash_key("console-test-key-cruda"))
        .execute(&pool)
        .await
        .expect("api key");

    Some(Harness {
        app: api::app(AppState { pool: pool.clone() }),
        pool,
        _turno: turno,
    })
}

impl Harness {
    async fn get(&self, uri: &str) -> (StatusCode, String, String) {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .body(Body::empty())
                    .expect("petición válida"),
            )
            .await
            .expect("respuesta del router");
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|valor| valor.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("cuerpo legible");
        (
            status,
            content_type,
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }
}

#[tokio::test]
async fn la_consola_se_sirve_bajo_console_con_fallback_de_spa() {
    let Some(h) = begin().await else { return };

    // El índice: HTML con el shell de la app (dist construido o stub).
    let (status, content_type, cuerpo) = h.get("/console").await;
    assert_eq!(status, StatusCode::OK, "{cuerpo}");
    assert!(content_type.starts_with("text/html"), "{content_type}");
    assert!(cuerpo.contains("Integraciones — consola"), "{cuerpo}");

    // El index.html explícito también responde.
    let (status, content_type, _) = h.get("/console/index.html").await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"));

    // Fallback SPA: cualquier ruta de la app devuelve el índice.
    let (status, content_type, cuerpo) = h.get("/console/executions/01a15568").await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"));
    assert!(cuerpo.contains("Integraciones — consola"));

    // Sin subida de directorio: la ruta rara cae al índice, no al filesystem.
    let (status, _, cuerpo) = h.get("/console/..%2f..%2fCargo.toml").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!cuerpo.contains("[dependencies]"), "{cuerpo}");

    // La consola no exige Bearer para servir el shell (los datos sí).
    // (El GET anterior no llevaba Authorization y fue 200.)
    let _ = h.pool;
}
