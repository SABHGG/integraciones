//! Tests de integración de la API de executions contra un Postgres **real**.
//!
//! Montan la misma aplicación axum que producción (`api::app`) y la ejercen a
//! través del router completo (auth Bearer incluida) con `tower::ServiceExt`:
//! listado con filtros y cursor, detalle con entrada del trigger y trace con
//! IO completo por bloque.
//!
//! Las ejecuciones se siembran por SQL (la cola y el ciclo pending→running→
//! succeeded|failed ya están cubiertos por los tests del engine, tareas 3.x).
//!
//! Si `DATABASE_URL` no está definida (CI sin base de datos) los tests se
//! omiten con un mensaje y terminan con éxito. En local:
//!
//! ```text
//! ./scripts/dev-postgres.sh
//! DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones cargo test -p api
//! ```

use api::auth::hash_key;
use api::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::{Mutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

/// Los tests comparten base de datos: se serializan para que la limpieza de
/// fixtures de un test no borre las filas de otro.
static DB_LOCK: Mutex<()> = Mutex::const_new(());

/// Respuesta ya leída: status, content-type y cuerpo crudo.
struct Resp {
    status: StatusCode,
    content_type: String,
    body: String,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("el cuerpo no es JSON ({e}): {}", self.body))
    }

    /// Exige un Problem Details (RFC 9457) con el status esperado y devuelve su cuerpo.
    fn problem(&self, status: StatusCode) -> Value {
        assert_eq!(self.status, status, "cuerpo: {}", self.body);
        assert_eq!(
            self.content_type, "application/problem+json",
            "todos los errores del plano admin son Problem Details: {}",
            self.body
        );
        let body = self.json();
        assert_eq!(body["status"], status.as_u16());
        assert_eq!(body["title"], status.canonical_reason().unwrap());
        body
    }
}

/// Aplicación + pool + key vinculada a un workspace de test.
struct Harness {
    app: Router,
    pool: PgPool,
    workspace_id: Uuid,
    key: String,
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

    // Limpia solo las filas creadas por estos tests (prefijo dedicado): los
    // workspaces se borran en cascada con sus bloques, puentes y keys.
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'exec-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces previos");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'exec-test%'")
        .execute(&pool)
        .await
        .expect("limpieza de keys previas");

    let workspace_id = Uuid::now_v7();
    let (key, _) = seed_workspace(&pool, workspace_id).await;

    let app = api::app(AppState { pool: pool.clone() });

    Some(Harness {
        app,
        pool,
        workspace_id,
        key,
        _turno: turno,
    })
}

/// Inserta un workspace y una API key vinculada; devuelve la key cruda.
async fn seed_workspace(pool: &PgPool, workspace_id: Uuid) -> (String, Uuid) {
    let slug = format!("exec-test-{workspace_id}");
    let key_id = Uuid::now_v7();
    let raw_key = format!("exec-test-key-{workspace_id}");

    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace de test")
        .bind(&slug)
        .execute(pool)
        .await
        .expect("workspace");

    sqlx::query("INSERT INTO api_keys (id, workspace_id, name, key_hash) VALUES ($1, $2, $3, $4)")
        .bind(key_id)
        .bind(workspace_id)
        .bind(format!("exec-test-{raw_key}"))
        .bind(hash_key(&raw_key))
        .execute(pool)
        .await
        .expect("api key");

    (raw_key, key_id)
}

impl Harness {
    async fn send(
        &self,
        method: Method,
        uri: &str,
        key: Option<&str>,
        body: Option<Value>,
    ) -> Resp {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(key) = key {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
        }
        let body = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };

        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).expect("petición válida"))
            .await
            .expect("respuesta del router");

        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("cuerpo legible");

        Resp {
            status,
            content_type,
            body: String::from_utf8(bytes.to_vec()).expect("cuerpo UTF-8"),
        }
    }

    async fn get(&self, uri: &str) -> Resp {
        self.send(Method::GET, uri, Some(&self.key), None).await
    }

    /// Crea un workspace extra con su key; devuelve `(workspace_id, key cruda)`.
    async fn extra_workspace(&self) -> (Uuid, String) {
        let id = Uuid::now_v7();
        let (key, _) = seed_workspace(&self.pool, id).await;
        (id, key)
    }

    /// Siembra un puente con una versión publicada (por SQL: la API de puentes
    /// se cubre en bridges_api.rs); devuelve `(bridgeId, bridgeVersionId)`.
    async fn bridge(&self, workspace_id: Uuid, suffix: &str) -> (Uuid, Uuid) {
        let bridge_id = Uuid::now_v7();
        let version_id = Uuid::now_v7();

        sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
            .bind(bridge_id)
            .bind(workspace_id)
            .bind(format!("exec-{suffix}"))
            .bind(format!("Puente {suffix}"))
            .execute(&self.pool)
            .await
            .expect("bridge");

        sqlx::query(
            "INSERT INTO bridge_versions (id, bridge_id, version, definition) VALUES ($1, $2, 1, $3)",
        )
        .bind(version_id)
        .bind(bridge_id)
        .bind(json!({ "trigger": { "type": "manual" }, "blocks": [] }))
        .execute(&self.pool)
        .await
        .expect("bridge_version");

        sqlx::query("UPDATE bridges SET latest_version = 1 WHERE id = $1")
            .bind(bridge_id)
            .execute(&self.pool)
            .await
            .expect("latest_version");

        (bridge_id, version_id)
    }

    /// Siembra una ejecución con fecha de alta explícita; devuelve su id.
    async fn execution(
        &self,
        workspace_id: Uuid,
        bridge: (Uuid, Uuid),
        status: &str,
        trigger_data: Value,
        error: Option<Value>,
        second: u32,
    ) -> Uuid {
        let id = Uuid::now_v7();
        let created_at = ts(second);
        let started = matches!(status, "running" | "succeeded" | "failed").then_some(created_at);
        let finished = matches!(status, "succeeded" | "failed").then_some(created_at);

        sqlx::query(
            "INSERT INTO executions (id, workspace_id, bridge_id, bridge_version_id, \
             trigger_type, trigger_data, status, error, run_at, started_at, finished_at, \
             created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'http', $5, $6, $7, $8, $9, $10, $11, $11)",
        )
        .bind(id)
        .bind(workspace_id)
        .bind(bridge.0)
        .bind(bridge.1)
        .bind(&trigger_data)
        .bind(status)
        .bind(&error)
        // v7 = ordenado por tiempo de creación de la fila; el run_at explícito
        // evita depender de cuándo se insertó.
        .bind(created_at)
        .bind(started)
        .bind(finished)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .expect("execution");

        id
    }

    /// Siembra un paso de trace con IO completo.
    async fn step(&self, execution_id: Uuid, seq: i32, status: &str, second: u32) {
        let created_at = ts(second);
        let output = matches!(status, "succeeded").then(|| json!({ "eco": seq }));
        let error = matches!(status, "failed")
            .then(|| json!({ "code": "SANDBOX_ERROR", "message": "boom" }));

        sqlx::query(
            "INSERT INTO execution_steps (execution_id, seq, block_instance_id, block_name, \
             status, input, output, error, duration_ms, created_at) \
             VALUES ($1, $2, 'paso', 'origen', $3, $4, $5, $6, 12, $7)",
        )
        .bind(execution_id)
        .bind(seq)
        .bind(status)
        .bind(json!({ "total": seq }))
        .bind(&output)
        .bind(&error)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .expect("step");
    }
}

/// Timestamp explícito para orden las filas de los fixtures.
fn ts(second: u32) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("2026-01-01T00:00:{second:02}Z"))
        .expect("timestamp de test")
        .with_timezone(&Utc)
}

#[tokio::test]
async fn lista_con_filtros_y_cursor() {
    let Some(h) = begin().await else { return };

    let bridge_a = h.bridge(h.workspace_id, "A").await;
    let bridge_b = h.bridge(h.workspace_id, "B").await;

    let failed = h
        .execution(
            h.workspace_id,
            bridge_b,
            "failed",
            json!({ "method": "GET" }),
            Some(json!({ "code": "SANDBOX_ERROR" })),
            10,
        )
        .await;

    let ok_2 = h
        .execution(h.workspace_id, bridge_a, "succeeded", json!({}), None, 20)
        .await;

    let ok_1 = h
        .execution(h.workspace_id, bridge_a, "succeeded", json!({}), None, 30)
        .await;

    // Sin filtros: 3 filas DESC por created_at con created_at de la fila.
    let lista = h.get("/api/v1/executions").await;
    assert_eq!(lista.status, StatusCode::OK, "{}", lista.body);
    let page = lista.json();
    assert_eq!(page["items"].as_array().unwrap().len(), 3);
    assert!(page["nextCursor"].is_null());
    assert_eq!(page["items"][0]["id"], ok_1.to_string());
    assert_eq!(page["items"][2]["id"], failed.to_string());

    // Filtro por puente.
    let lista = h
        .get(&format!("/api/v1/executions?bridgeId={}", bridge_a.0))
        .await;
    let page = lista.json();
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert!(page["items"][0]["status"].is_string());

    // Filtro por estado.
    let lista = h.get("/api/v1/executions?status=failed").await;
    let page = lista.json();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["id"], failed.to_string());
    let lista = h.get("/api/v1/executions?status=succeeded").await;
    assert_eq!(lista.json()["items"].as_array().unwrap().len(), 2);

    // Filtro por fecha: desde 00:00:15, quedan las dos del bridge A.
    let lista = h.get("/api/v1/executions?from=2026-01-01T00:00:15Z").await;
    assert_eq!(lista.json()["items"].as_array().unwrap().len(), 2);
    let lista = h
        .get("/api/v1/executions?from=2026-01-01T00:00:15Z&to=2026-01-01T00:00:25Z")
        .await;
    let page = lista.json();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["id"], ok_2.to_string());

    // Filtro inválido → Problem Details 400.
    h.get("/api/v1/executions?status=rara")
        .await
        .problem(StatusCode::BAD_REQUEST);
    h.get("/api/v1/executions?bridgeId=no-es-uuid")
        .await
        .problem(StatusCode::BAD_REQUEST);
    h.get("/api/v1/executions?from=no-es-fecha")
        .await
        .problem(StatusCode::BAD_REQUEST);

    // Cursor con limit=1: tres páginas encadenadas.
    let primera = h.get("/api/v1/executions?limit=1").await.json();
    assert_eq!(primera["items"].as_array().unwrap().len(), 1);
    let cursor_a = primera["nextCursor"].as_str().expect("cursor de página 1");

    let segunda = h
        .get(&format!("/api/v1/executions?limit=1&cursor={cursor_a}"))
        .await
        .json();
    assert_eq!(segunda["items"][0]["id"], ok_2.to_string());
    let cursor_b = segunda["nextCursor"].as_str().expect("cursor de página 2");
    assert_ne!(cursor_a, cursor_b);

    let tercera = h
        .get(&format!("/api/v1/executions?limit=1&cursor={cursor_b}"))
        .await
        .json();
    assert_eq!(tercera["items"][0]["id"], failed.to_string());
    assert!(tercera["nextCursor"].is_null(), "última página sin cursor");

    // Cursor aceptado compartiendo filtros (status+fecha+bridge).
    let lista = h
        .get("/api/v1/executions?status=succeeded&limit=1&from=2026-01-01T00:00:15Z")
        .await
        .json();
    let con_cursor = h
        .get(&format!(
            "/api/v1/executions?status=succeeded&limit=1&from=2026-01-01T00:00:15Z&cursor={}",
            lista["nextCursor"].as_str().expect("cursor con filtros")
        ))
        .await
        .json();
    assert_eq!(
        con_cursor["items"][0]["id"],
        ok_2.to_string(),
        "{con_cursor:?}"
    );
}

#[tokio::test]
async fn obtiene_el_detalle_con_entrada_del_trigger_y_version() {
    let Some(h) = begin().await else { return };

    let (bridge, version) = h.bridge(h.workspace_id, "detalle").await;
    let id = h
        .execution(
            h.workspace_id,
            (bridge, version),
            "succeeded",
            json!({ "method": "POST", "path": "/x/demo", "body": { "total": 3 } }),
            None,
            5,
        )
        .await;

    // Detalle: estado, fechas, entrada del trigger y versión con la que corrió.
    let respuesta = h.get(&format!("/api/v1/executions/{id}")).await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let detalle = respuesta.json();
    assert_eq!(detalle["id"], id.to_string());
    assert_eq!(detalle["bridgeId"], bridge.to_string());
    assert!(
        detalle["bridgeVersionId"].as_str().is_some(),
        "la versión exacta está"
    );
    assert_eq!(detalle["triggerType"], "http");
    assert_eq!(detalle["status"], "succeeded");
    assert_eq!(
        detalle["triggerData"]["body"]["total"], 3,
        "entrada del trigger"
    );
    assert!(detalle["startedAt"].is_string());
    assert!(detalle["finishedAt"].is_string());
    assert!(detalle["error"].is_null());

    // La ejecución de otra workspace no existe (ni por UUID conocido).
    let (workspace_id, other_key) = h.extra_workspace().await;
    assert_ne!(workspace_id, h.workspace_id);
    let respuesta = h
        .send(
            Method::GET,
            &format!("/api/v1/executions/{id}"),
            Some(&other_key),
            None,
        )
        .await;
    respuesta.problem(StatusCode::NOT_FOUND);

    // Id no-UUID → 404 sin panic.
    h.get("/api/v1/executions/no-es-uuid")
        .await
        .problem(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn el_trace_devuelve_los_pasos_en_orden_con_io_duracion_y_fallo() {
    let Some(h) = begin().await else { return };

    let (bridge, version) = h.bridge(h.workspace_id, "trace").await;
    let id = h
        .execution(
            h.workspace_id,
            (bridge, version),
            "failed",
            json!({}),
            Some(json!({ "code": "SANDBOX_ERROR", "message": "boom en el paso 2" })),
            5,
        )
        .await;

    h.step(id, 1, "succeeded", 6).await;
    h.step(id, 2, "failed", 7).await;

    let respuesta = h.get(&format!("/api/v1/executions/{id}/trace")).await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let trace = respuesta.json();
    assert_eq!(trace["executionId"], id.to_string());

    let steps = trace["steps"].as_array().expect("pasos");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["seq"], 1);
    assert_eq!(steps[0]["blockName"], "origen");
    assert_eq!(steps[0]["status"], "succeeded");
    assert_eq!(steps[0]["input"]["total"], 1, "entrada resuelta");
    assert_eq!(steps[0]["output"]["eco"], 1, "salida completa");
    assert!(steps[0]["error"].is_null());
    assert_eq!(steps[0]["durationMs"], 12, "duración por bloque");
    assert!(steps[0]["createdAt"].is_string());

    // El punto exacto del fallo: paso 2 con error estructurado y output null.
    assert_eq!(steps[1]["seq"], 2);
    assert_eq!(steps[1]["status"], "failed");
    assert!(
        steps[1]["error"]["code"].as_str().is_some(),
        "error del paso"
    );
    let detalle = h.get(&format!("/api/v1/executions/{id}")).await.json();
    assert!(
        detalle["error"]["code"].as_str().is_some(),
        "la ejecución cae con error estructurado: {detalle}"
    );

    // Ejecución sin pasos (nunca reclamada): trace vacío.
    let bridge_vacia = h.bridge(h.workspace_id, "vacia").await;
    let pendiente = h
        .execution(h.workspace_id, bridge_vacia, "pending", json!({}), None, 9)
        .await;
    let respuesta = h
        .get(&format!("/api/v1/executions/{pendiente}/trace"))
        .await;
    assert_eq!(respuesta.status, StatusCode::OK);
    let pendiente_json = respuesta.json();
    assert_eq!(pendiente_json["steps"], json!([]), "sin pasos, sin panic");

    // Otra workspace: 404.
    let (workspace_id, other_key) = h.extra_workspace().await;
    assert_ne!(workspace_id, h.workspace_id);
    let respuesta = h
        .send(
            Method::GET,
            &format!("/api/v1/executions/{id}/trace"),
            Some(&other_key),
            None,
        )
        .await;
    respuesta.problem(StatusCode::NOT_FOUND);
}
