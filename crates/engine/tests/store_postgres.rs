//! Tests de integración del store contra un Postgres **real**.
//!
//! Si `DATABASE_URL` no está definida (CI sin base de datos) los tests se
//! omiten con un mensaje y terminan con éxito. En local:
//!
//! ```text
//! ./scripts/dev-postgres.sh
//! DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones cargo test -p engine
//! ```

use domain::definition::TriggerType;
use domain::execution::{ErrorCode, ErrorPayload, ExecutionStatus, StepStatus};
use engine::executor::{ExecutionOutcome, ExecutionStep};
use engine::store::{ExecutionStore, NewExecution, StoreError};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::{Mutex, MutexGuard};
use uuid::Uuid;

/// Los tests comparten base de datos: se serializan para que el claim global
/// (`FOR UPDATE SKIP LOCKED`) sea determinista entre ellos.
static DB_LOCK: Mutex<()> = Mutex::const_new(());

/// Abre el pool, aplica migraciones y toma el turno. `None` = sin BD.
async fn begin() -> Option<(PgPool, MutexGuard<'static, ()>)> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!(
                "DATABASE_URL no está definida: se omiten los tests de integración del store"
            );
            return None;
        }
    };
    // La cola de ejecuciones es GLOBAL (claim_next cruza workspaces), así que
    // estos tests usan una base de datos dedicada: si compartieran la BD con
    // los tests de la API, un worker podría reclamar ejecuciones `pending`
    // sembradas por otro binario de pruebas (carrera intermitente).
    let url = url
        .rsplit_once('/')
        .map(|(base, _db)| format!("{base}/integraciones_engine"))
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

    // Limpia solo las filas creadas por estos tests (prefijo de slug dedicado).
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'engine-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de fixtures previos");

    Some((pool, guard))
}

/// Fixtures mínimas para satisfacer las claves foráneas de `executions`.
struct Fixture {
    workspace_id: Uuid,
    bridge_id: Uuid,
    bridge_version_id: Uuid,
}

async fn fixture(pool: &PgPool) -> Fixture {
    let workspace_id = Uuid::now_v7();
    let bridge_id = Uuid::now_v7();
    let bridge_version_id = Uuid::now_v7();
    let slug = format!("engine-test-{workspace_id}");

    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace de test")
        .bind(&slug)
        .execute(pool)
        .await
        .expect("workspace");

    sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
        .bind(bridge_id)
        .bind(workspace_id)
        .bind("demo")
        .bind("Puente demo")
        .execute(pool)
        .await
        .expect("bridge");

    sqlx::query(
        "INSERT INTO bridge_versions (id, bridge_id, version, definition) VALUES ($1, $2, 1, $3)",
    )
    .bind(bridge_version_id)
    .bind(bridge_id)
    .bind(json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/demo" },
        "blocks": []
    }))
    .execute(pool)
    .await
    .expect("bridge_version");

    Fixture {
        workspace_id,
        bridge_id,
        bridge_version_id,
    }
}

fn new_execution(fixture: &Fixture) -> NewExecution {
    NewExecution {
        workspace_id: fixture.workspace_id,
        bridge_id: fixture.bridge_id,
        bridge_version_id: fixture.bridge_version_id,
        trigger_type: TriggerType::Http,
        trigger_data: json!({ "body": { "userId": "u-1" }, "query": {} }),
        idempotency_key: None,
    }
}

fn succeeded_step(seq: i32, name: &str, input: Value, output: Value) -> ExecutionStep {
    ExecutionStep {
        seq,
        block_instance_id: format!("instancia-{name}"),
        block_name: name.to_string(),
        status: StepStatus::Succeeded,
        input,
        output: Some(output),
        error: None,
        duration_ms: Some(3),
    }
}

fn succeeded_outcome(steps: Vec<ExecutionStep>) -> ExecutionOutcome {
    let output = steps.last().and_then(|step| step.output.clone());
    ExecutionOutcome {
        status: ExecutionStatus::Succeeded,
        output,
        error: None,
        context: json!({ "now": "2025-01-02T03:04:05Z" }),
        steps,
    }
}

/// Estado y error persistidos en `executions`.
async fn execution_state(pool: &PgPool, id: Uuid) -> (String, Option<Value>, bool) {
    let (status, error, finished): (String, Option<Value>, bool) = sqlx::query_as(
        "SELECT status, error, finished_at IS NOT NULL FROM executions WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("fila de executions");
    (status, error, finished)
}

/// Pasos persistidos, en orden de secuencia.
async fn persisted_steps(
    pool: &PgPool,
    id: Uuid,
) -> Vec<(
    i32,
    String,
    String,
    Value,
    Option<Value>,
    Option<Value>,
    Option<i32>,
)> {
    sqlx::query_as(
        "SELECT seq, block_instance_id, block_name, input, output, error, duration_ms
           FROM execution_steps
          WHERE execution_id = $1
          ORDER BY seq",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .expect("pasos de la ejecución")
}

// ── create ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_deja_la_ejecucion_pending_con_uuid_v7_y_sin_pasos() {
    let Some((pool, _turno)) = begin().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let store = ExecutionStore::new(pool.clone());

    let row = store
        .create(new_execution(&fixture))
        .await
        .expect("crear ejecución");

    assert_eq!(row.status, "pending");
    assert_eq!(row.status_typed(), Some(ExecutionStatus::Pending));
    assert_eq!(row.id.get_version_num(), 7, "el id debe ser UUID v7");
    assert_eq!(row.trigger_type, "http");
    assert_eq!(row.bridge_version_id, fixture.bridge_version_id);
    assert_eq!(
        row.trigger_data,
        json!({ "body": { "userId": "u-1" }, "query": {} })
    );
    assert!(row.started_at.is_none());
    assert!(row.finished_at.is_none());
    assert!(row.error.is_none());
    assert!(persisted_steps(&pool, row.id).await.is_empty());
}

// ── claim ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn claim_pasa_pending_a_running_una_sola_vez() {
    let Some((pool, _turno)) = begin().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let store = ExecutionStore::new(pool.clone());

    let created = store.create(new_execution(&fixture)).await.expect("crear");

    let claimed = store
        .claim_next()
        .await
        .expect("claim")
        .expect("hay una ejecución pendiente");
    assert_eq!(claimed.id, created.id);
    assert_eq!(claimed.status, "running");
    assert_eq!(claimed.status_typed(), Some(ExecutionStatus::Running));
    assert!(claimed.started_at.is_some(), "el claim marca started_at");

    assert!(
        store.claim_next().await.expect("segundo claim").is_none(),
        "una ejecución reclamada no se vuelve a entregar"
    );
}

#[tokio::test]
async fn dos_workers_concurrentes_no_reciben_la_misma_ejecucion() {
    let Some((pool, _turno)) = begin().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let store_a = ExecutionStore::new(pool.clone());
    let store_b = ExecutionStore::new(pool.clone());

    let created = store_a
        .create(new_execution(&fixture))
        .await
        .expect("crear");

    let (first, second) = tokio::join!(store_a.claim_next(), store_b.claim_next());
    let claimed: Vec<Uuid> = [first, second]
        .into_iter()
        .filter_map(|result| result.expect("claim sin error"))
        .map(|row| row.id)
        .collect();

    assert_eq!(
        claimed,
        vec![created.id],
        "exactamente un worker la reclama"
    );
    assert!(store_a.claim_next().await.expect("cola vacía").is_none());
}

// ── finish ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn finish_persiste_todos_los_pasos_y_el_estado_final_en_una_transaccion() {
    let Some((pool, _turno)) = begin().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let store = ExecutionStore::new(pool.clone());

    let created = store.create(new_execution(&fixture)).await.expect("crear");
    store.claim_next().await.expect("claim").expect("reclamada");

    let outcome = succeeded_outcome(vec![
        succeeded_step(1, "usuario", json!({"id": "u-1"}), json!({"nombre": "ana"})),
        succeeded_step(2, "pedido", json!({"nombre": "ana"}), json!({"total": 20})),
    ]);
    store
        .finish(created.id, &outcome)
        .await
        .expect("cerrar ejecución");

    let (status, error, finished) = execution_state(&pool, created.id).await;
    assert_eq!(status, "succeeded");
    assert!(error.is_none());
    assert!(finished, "finished_at queda sellado");

    let steps = persisted_steps(&pool, created.id).await;
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].0, 1);
    assert_eq!(steps[0].1, "instancia-usuario");
    assert_eq!(steps[0].2, "usuario");
    assert_eq!(steps[0].3, json!({"id": "u-1"}));
    assert_eq!(steps[0].4, Some(json!({"nombre": "ana"})));
    assert!(steps[0].5.is_none());
    assert_eq!(steps[0].6, Some(3));
    assert_eq!(steps[1].0, 2);
    assert_eq!(steps[1].2, "pedido");
    assert_eq!(steps[1].4, Some(json!({"total": 20})));
}

#[tokio::test]
async fn finish_con_error_persiste_el_detalle_en_la_ejecucion_y_en_el_paso() {
    let Some((pool, _turno)) = begin().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let store = ExecutionStore::new(pool.clone());

    let created = store.create(new_execution(&fixture)).await.expect("crear");
    store.claim_next().await.expect("claim").expect("reclamada");

    let error = ErrorPayload::new(
        ErrorCode::BlockFailed,
        "el bloque \"rompe\" lanzó un error: boom",
    )
    .with_block("rompe")
    .with_details(json!({ "reason": "JS_ERROR", "message": "boom" }));
    let failed_step = ExecutionStep {
        seq: 2,
        block_instance_id: "instancia-rompe".to_string(),
        block_name: "rompe".to_string(),
        status: StepStatus::Failed,
        input: json!({ "ok": true }),
        output: None,
        error: Some(error.clone()),
        duration_ms: None,
    };
    let outcome = ExecutionOutcome {
        status: ExecutionStatus::Failed,
        output: None,
        error: Some(error),
        context: json!({}),
        steps: vec![
            succeeded_step(1, "ok", json!({}), json!({ "ok": true })),
            failed_step,
        ],
    };

    store.finish(created.id, &outcome).await.expect("cerrar");

    let (status, persisted_error, finished) = execution_state(&pool, created.id).await;
    assert_eq!(status, "failed");
    assert!(finished);
    let persisted_error = persisted_error.expect("error estructurado persistido");
    assert_eq!(persisted_error["code"], json!("BLOCK_FAILED"));
    assert_eq!(persisted_error["block"], json!("rompe"));
    assert_eq!(persisted_error["details"]["reason"], json!("JS_ERROR"));

    let steps = persisted_steps(&pool, created.id).await;
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[1].2, "rompe");
    assert!(steps[1].4.is_none(), "el paso fallido no tiene salida");
    assert_eq!(steps[1].5.as_ref().unwrap()["code"], json!("BLOCK_FAILED"));
    assert!(steps[1].6.is_none(), "no se invocó el sandbox");
}

#[tokio::test]
async fn finish_es_atomico_si_un_paso_viola_un_constraint() {
    let Some((pool, _turno)) = begin().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let store = ExecutionStore::new(pool.clone());

    let created = store.create(new_execution(&fixture)).await.expect("crear");
    store.claim_next().await.expect("claim").expect("reclamada");

    // Dos pasos con la misma secuencia: viola UNIQUE (execution_id, seq) a mitad
    // de la transacción. Nada debe quedar escrito.
    let outcome = succeeded_outcome(vec![
        succeeded_step(1, "uno", json!({}), json!({ "a": 1 })),
        succeeded_step(1, "dos", json!({}), json!({ "b": 2 })),
    ]);

    let result = store.finish(created.id, &outcome).await;
    assert!(
        matches!(result, Err(StoreError::Database(_))),
        "se esperaba error de base de datos: {result:?}"
    );

    let (status, error, finished) = execution_state(&pool, created.id).await;
    assert_eq!(status, "running", "el estado no avanza si el cierre falla");
    assert!(error.is_none());
    assert!(!finished);
    assert!(
        persisted_steps(&pool, created.id).await.is_empty(),
        "la transacción no deja pasos parciales"
    );
}

#[tokio::test]
async fn finish_de_una_ejecucion_no_reclamada_es_un_error_explicito() {
    let Some((pool, _turno)) = begin().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let store = ExecutionStore::new(pool.clone());

    let created = store.create(new_execution(&fixture)).await.expect("crear");

    let result = store.finish(created.id, &succeeded_outcome(vec![])).await;

    match result {
        Err(StoreError::NotRunning { id }) => assert_eq!(id, created.id),
        other => panic!("se esperaba NotRunning, llegó {other:?}"),
    }

    let (status, _, finished) = execution_state(&pool, created.id).await;
    assert_eq!(status, "pending");
    assert!(!finished);
}
