//! Tests del scheduler de triggers cron (MVP2, tasks 5.1-5.3) contra un
//! Postgres **real**.
//!
//! El reloj es inyectable: `api::scheduler::tick(&state, ahora)` y
//! `recuperar_arranque(&state, ahora)` reciben el instante, así que los tests
//! avanzan el tiempo de forma determinista sin esperar nada.
//!
//! Corren en una base de datos **dedicada** (`integraciones_scheduler`): la
//! cola de ejecuciones es global (SKIP LOCKED) y otras suites siembran filas
//! `pending` en paralelo.
//!
//! Si `DATABASE_URL` no está definida (CI sin base de datos) los tests se
//! omiten con un mensaje y terminan con éxito.

use api::state::AppState;
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::{Mutex, MutexGuard};
use uuid::Uuid;

/// Los tests comparten base de datos: se serializan.
static DB_LOCK: Mutex<()> = Mutex::const_new(());

async fn begin() -> Option<(AppState, MutexGuard<'static, ()>)> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!(
                "DATABASE_URL no está definida: se omiten los tests del scheduler"
            );
            return None;
        }
    };
    let url = url
        .rsplit_once('/')
        .map(|(base, _db)| format!("{base}/integraciones_scheduler"))
        .expect("DATABASE_URL con nombre de base de datos");

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("conexión a Postgres (¿corriste ./scripts/dev-postgres.sh?)");

    sqlx::migrate!("../api/migrations")
        .run(&pool)
        .await
        .expect("migraciones aplicadas");

    let guard = DB_LOCK.lock().await;

    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'sched-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de fixtures previos");

    Some((AppState { pool }, guard))
}

/// Siembra un puente publicado con trigger cron (`* * * * *` cada minuto en
/// la timezone dada, o la expresión y zona indicadas).
async fn puente_cron(pool: &PgPool, slug: &str, schedule: &str, timezone: &str) -> Uuid {
    let workspace_id = Uuid::now_v7();
    let bridge_id = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace scheduler")
        .bind(format!("sched-test-{workspace_id}"))
        .execute(pool)
        .await
        .expect("workspace");
    sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
        .bind(bridge_id)
        .bind(workspace_id)
        .bind(slug)
        .bind(format!("Puente {slug}"))
        .execute(pool)
        .await
        .expect("bridge");
    let definition = json!({
        "trigger": { "type": "cron", "schedule": schedule, "timezone": timezone },
        "blocks": [{
            "id": "b1",
            "name": "eco",
            "code": "export default function(input, ctx) { return { ok: true }; }",
            "input": {},
            "config": {},
            "outputSchema": {}
        }]
    });
    sqlx::query(
        "INSERT INTO bridge_versions (id, bridge_id, version, definition) VALUES ($1, $2, 1, $3)",
    )
    .bind(Uuid::now_v7())
    .bind(bridge_id)
    .bind(&definition)
    .execute(pool)
    .await
    .expect("bridge_version");
    sqlx::query("UPDATE bridges SET latest_version = 1 WHERE id = $1")
        .bind(bridge_id)
        .execute(pool)
        .await
        .expect("latest_version");
    bridge_id
}

/// Filas cron del puente: (scheduled_at, status, run_at).
async fn ejecuciones_cron(pool: &PgPool, bridge_id: Uuid) -> Vec<(Value, String, Value)> {
    sqlx::query_as(
        "SELECT trigger_data->'scheduled_at', status, to_jsonb(run_at) \
         FROM executions \
         WHERE bridge_id = $1 AND trigger_type = 'cron' \
         ORDER BY (trigger_data->>'scheduled_at')",
    )
    .bind(bridge_id)
    .fetch_all(pool)
    .await
    .expect("ejecuciones cron")
}

/// Motivos de las ocurrencias no ejecutadas del puente.
async fn missed(pool: &PgPool, bridge_id: Uuid) -> Vec<(Value, String)> {
    sqlx::query_as(
        "SELECT to_jsonb(scheduled_at), reason FROM missed_triggers \
         WHERE bridge_id = $1 ORDER BY scheduled_at",
    )
    .bind(bridge_id)
    .fetch_all(pool)
    .await
    .expect("missed_triggers")
}

fn at(instante: &str) -> chrono::DateTime<Utc> {
    chrono::NaiveDateTime::parse_from_str(instante, "%Y-%m-%dT%H:%M:%S")
        .expect("instante de prueba")
        .and_utc()
}

#[tokio::test]
async fn el_scheduler_pre_encola_con_run_at_programado_y_sin_duplicar() {
    let Some((state, _turno)) = begin().await else { return };
    let bridge_id = puente_cron(&state.pool, "cada-minuto", "* * * * *", "UTC").await;

    // 5.1: tick a las 09:03:30 → pre-encola la ocurrencia de las 09:04 con
    // run_at = 09:04 (el worker la reclama cuando vence, no antes).
    let ahora = at("2026-03-04T09:03:30");
    let acciones = api::scheduler::tick(&state, ahora).await.expect("tick");
    assert_eq!(acciones, 1);

    let filas = ejecuciones_cron(&state.pool, bridge_id).await;
    assert_eq!(filas.len(), 1);
    assert_eq!(filas[0].0, json!("2026-03-04T09:04:00+00:00"));
    assert_eq!(filas[0].1, "pending");

    // El mismo tick no duplica: la 09:04 ya está registrada.
    api::scheduler::tick(&state, ahora).await.expect("tick 2");
    assert_eq!(ejecuciones_cron(&state.pool, bridge_id).await.len(), 1);

    // La ocurrencia termina; el tick siguiente programa la posterior.
    sqlx::query(
        "UPDATE executions SET status = 'succeeded', finished_at = now() \
         WHERE bridge_id = $1 AND trigger_type = 'cron'",
    )
    .bind(bridge_id)
    .execute(&state.pool)
    .await
    .expect("cerrar la ejecución");
    api::scheduler::tick(&state, at("2026-03-04T09:04:30"))
        .await
        .expect("tick 3");
    let filas = ejecuciones_cron(&state.pool, bridge_id).await;
    assert_eq!(filas.len(), 2);
    assert_eq!(filas[1].0, json!("2026-03-04T09:05:00+00:00"));
    assert_eq!(filas[1].1, "pending");
}

#[tokio::test]
async fn un_puente_lento_registra_overlap_y_no_acumula_ejecuciones() {
    let Some((state, _turno)) = begin().await else { return };
    let bridge_id = puente_cron(&state.pool, "lento", "* * * * *", "UTC").await;

    // La 09:05 queda pre-encolada y sigue `pending` (nadie la corre).
    api::scheduler::tick(&state, at("2026-03-04T09:04:30"))
        .await
        .expect("tick inicial");

    // 09:07: la 09:06 ya venció con la 09:05 viva → overlap, sin encolar.
    api::scheduler::tick(&state, at("2026-03-04T09:07:30"))
        .await
        .expect("tick solape");
    assert_eq!(
        ejecuciones_cron(&state.pool, bridge_id).await.len(),
        1,
        "una ejecución lenta no acumula una segunda"
    );
    let perdidas = missed(&state.pool, bridge_id).await;
    assert_eq!(perdidas.len(), 1);
    assert_eq!(perdidas[0].1, "overlap");
    assert_eq!(perdidas[0].0, json!("2026-03-04T09:06:00+00:00"));

    // Cada minuto que pasa con la ejecución viva registra su overlap.
    api::scheduler::tick(&state, at("2026-03-04T09:08:30"))
        .await
        .expect("tick solape 2");
    assert_eq!(missed(&state.pool, bridge_id).await.len(), 2);

    // Al terminar la ejecución, el flujo se reanuda con la siguiente ocurrencia.
    sqlx::query(
        "UPDATE executions SET status = 'succeeded', finished_at = now() \
         WHERE bridge_id = $1 AND trigger_type = 'cron'",
    )
    .bind(bridge_id)
    .execute(&state.pool)
    .await
    .expect("cerrar");
    api::scheduler::tick(&state, at("2026-03-04T09:09:30"))
        .await
        .expect("tick reanudación");
    let filas = ejecuciones_cron(&state.pool, bridge_id).await;
    assert_eq!(filas.len(), 2, "la ejecución viva se cerró: se encola la siguiente vencida");
    assert_eq!(filas[1].0, json!("2026-03-04T09:08:00+00:00"));
}

#[tokio::test]
async fn la_recuperacion_tras_una_caida_encola_solo_la_reciente_y_registra_downtime() {
    let Some((state, _turno)) = begin().await else { return };
    let bridge_id = puente_cron(&state.pool, "recuperable", "* * * * *", "UTC").await;

    // Historial: la 09:00 corrió bien. El proceso cae y reaparece a las 10:05:
    // vencen 09:01..10:05 (65 ocurrencias) sin registro alguno.
    let (workspace_id, version_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT b.workspace_id, bv.id \
         FROM bridges b \
         JOIN bridge_versions bv ON bv.bridge_id = b.id AND bv.version = b.latest_version \
         WHERE b.id = $1",
    )
    .bind(bridge_id)
    .fetch_one(&state.pool)
    .await
    .expect("fixture");
    sqlx::query(
        "INSERT INTO executions \
             (id, workspace_id, bridge_id, bridge_version_id, trigger_type, trigger_data, \
              status, run_at, finished_at) \
         VALUES ($1, $2, $3, $4, 'cron', $5, 'succeeded', $6, now())",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(bridge_id)
    .bind(version_id)
    .bind(json!({ "scheduled_at": "2026-03-04T09:00:00+00:00" }))
    .bind(at("2026-03-04T09:00:00"))
    .execute(&state.pool)
    .await
    .expect("ejecución histórica");

    // Recuperación a las 10:05: encola solo la 10:05; 09:01..10:04 downtime.
    let escritas = api::scheduler::recuperar_arranque(&state, at("2026-03-04T10:05:00"))
        .await
        .expect("recuperación");
    assert_eq!(escritas, 65, "64 downtime + 1 encolada");

    let filas = ejecuciones_cron(&state.pool, bridge_id).await;
    // La histórica de las 09:00 (succeeded) + la 10:05 recién encolada.
    assert_eq!(filas.len(), 2);
    assert_eq!(filas[0].0, json!("2026-03-04T09:00:00+00:00"));
    assert_eq!(filas[0].1, "succeeded");
    assert_eq!(filas[1].0, json!("2026-03-04T10:05:00+00:00"));
    assert_eq!(filas[1].1, "pending");

    let perdidas = missed(&state.pool, bridge_id).await;
    assert_eq!(perdidas.len(), 64);
    assert!(perdidas.iter().all(|(_, reason)| reason == "downtime"));
    assert_eq!(perdidas[0].0, json!("2026-03-04T09:01:00+00:00"));
    assert_eq!(perdidas[63].0, json!("2026-03-04T10:04:00+00:00"));

    // Segunda recuperación (sin caída nueva): no escribe nada.
    let escritas = api::scheduler::recuperar_arranque(&state, at("2026-03-04T10:05:30"))
        .await
        .expect("recuperación 2");
    assert_eq!(escritas, 0);
}
