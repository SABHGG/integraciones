//! Tests del job de retención (task 5.3) contra un Postgres **real**.
//!
//! Cubre el spec `execution-store`: purga del IO de `execution_steps` a los
//! 30 días manteniendo el resumen de `executions` consultable hasta su
//! ventana anual, y purga final de los resúmenes. Los umbrales son los
//! argumentos de `retention::purge`; el job de fondo solo es su bucle.
//!
//! Si `DATABASE_URL` no está definida (CI sin base de datos) los tests se
//! omiten con un mensaje y terminan con éxito. En local:
//!
//! ```text
//! ./scripts/dev-postgres.sh
//! DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones cargo test -p api --test retention_api
//! ```

use chrono::{Duration, Utc};
use sqlx::PgPool;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use uuid::Uuid;

/// Los tests comparten base de datos: se serializan para que la limpieza de
/// fixtures de un test no borre las filas de otro.
static DB_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// Prefijo exclusivo de estos tests (limpieza por cascada de workspaces).
const WS_PREFIX: &str = "ret-test-";

async fn begin() -> Option<(PgPool, MutexGuard<'static, ()>, Uuid)> {
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

    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'ret-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces previos");

    let workspace_id = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace de test")
        .bind(format!("{WS_PREFIX}{workspace_id}"))
        .execute(&pool)
        .await
        .expect("workspace");

    Some((pool, turno, workspace_id))
}

/// Siembra puente v1 + una ejecución con `edad_dias` y `pasos` steps de la
/// misma antigüedad; devuelve el id de la ejecución.
async fn siembra(pool: &PgPool, workspace_id: Uuid, suffix: &str, edad_dias: i64, pasos: usize) -> Uuid {
    let bridge_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let execution_id = Uuid::now_v7();
    let created_at = Utc::now() - Duration::days(edad_dias);

    sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
        .bind(bridge_id)
        .bind(workspace_id)
        .bind(format!("ret-{suffix}"))
        .bind(format!("Puente {suffix}"))
        .execute(pool)
        .await
        .expect("bridge");

    sqlx::query(
        "INSERT INTO bridge_versions (id, bridge_id, version, definition) VALUES ($1, $2, 1, $3)",
    )
    .bind(version_id)
    .bind(bridge_id)
    .bind(serde_json::json!({ "trigger": { "type": "manual" }, "blocks": [] }))
    .execute(pool)
    .await
    .expect("bridge_version");

    sqlx::query(
        "INSERT INTO executions (id, workspace_id, bridge_id, bridge_version_id, \
         trigger_type, trigger_data, status, run_at, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, 'http', $5, 'succeeded', $6, $6, $6)",
    )
    .bind(execution_id)
    .bind(workspace_id)
    .bind(bridge_id)
    .bind(version_id)
    .bind(serde_json::json!({}))
    .bind(created_at)
    .execute(pool)
    .await
    .expect("execution");

    for seq in 1..=pasos {
        sqlx::query(
            "INSERT INTO execution_steps (execution_id, seq, block_instance_id, block_name, \
             status, input, output, duration_ms, created_at) \
             VALUES ($1, $2, 'paso', 'origen', 'succeeded', $3, $3, 12, $4)",
        )
        .bind(execution_id)
        .bind(seq as i32)
        .bind(serde_json::json!({ "total": seq }))
        .bind(created_at)
        .execute(pool)
        .await
        .expect("step");
    }

    execution_id
}

async fn contar(pool: &PgPool, sql: &str, execution_id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(execution_id)
        .fetch_one(pool)
        .await
        .expect("conteo")
}

#[tokio::test]
async fn purga_el_io_viejo_y_mantiene_el_resumen_hasta_su_ventana() {
    let Some((pool, _turno, workspace_id)) = begin().await else { return };

    let vieja = siembra(&pool, workspace_id, "vieja", 40, 2).await;
    let reciente = siembra(&pool, workspace_id, "reciente", 0, 1).await;

    let reporte = api::retention::purge(&pool, 30, 365)
        .await
        .expect("purga");
    assert_eq!(reporte.steps, 2, "solo el IO de la vieja");
    assert_eq!(reporte.executions, 0, "el resumen de la vieja sigue en su ventana");

    assert_eq!(
        contar(
            &pool,
            "SELECT count(*) FROM execution_steps WHERE execution_id = $1",
            vieja
        )
        .await,
        0,
        "el IO viejo se purgó"
    );
    assert_eq!(
        contar(
            &pool,
            "SELECT count(*) FROM execution_steps WHERE execution_id = $1",
            reciente
        )
        .await,
        1,
        "el IO reciente se conserva"
    );
    assert_eq!(
        contar(&pool, "SELECT count(*) FROM executions WHERE id = $1", vieja).await,
        1,
        "el resumen viejo sigue consultable"
    );

    // Idempotente: la segunda pasada no encuentra nada nuevo.
    let reporte = api::retention::purge(&pool, 30, 365)
        .await
        .expect("segunda purga");
    assert_eq!(reporte.steps, 0);
    assert_eq!(reporte.executions, 0);
}

#[tokio::test]
async fn purga_los_resumenes_pasada_su_ventana_anual() {
    let Some((pool, _turno, workspace_id)) = begin().await else { return };

    let viejisima = siembra(&pool, workspace_id, "anual", 400, 3).await;

    let reporte = api::retention::purge(&pool, 30, 365)
        .await
        .expect("purga");
    assert_eq!(reporte.steps, 3, "los pasos de la viejísima también salen");
    assert_eq!(reporte.executions, 1, "el resumen sale al cumplir el año");

    assert_eq!(
        contar(&pool, "SELECT count(*) FROM executions WHERE id = $1", viejisima).await,
        0
    );
}
