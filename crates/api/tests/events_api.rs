//! Tests de eventos: suscripción por topic, fan-out transaccional y emisión
//! al completar (MVP3, tasks 3.1-3.3, spec `event-triggers`).
//!
//! Los puentes se siembran por SQL (snapshots publicados) para controlar el
//! trigger congelado; el fan-out se verifica contra la cola (`executions`
//! `pending`) y se drena con el paso determinista del worker.
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
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

/// Los tests comparten base de datos: se serializan.
static DB_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// Prefijo exclusivo de estos tests (limpieza por cascada de workspaces).
const WS_PREFIX: &str = "event-test-";

/// Respuesta ya leída: status y cuerpo crudo.
struct Resp {
    status: StatusCode,
    body: String,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("el cuerpo no es JSON ({e}): {}", self.body))
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

    // Limpia solo las filas creadas por estos tests (prefijo dedicado).
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'event-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces previos");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'event-test%'")
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
    let slug = format!("{WS_PREFIX}{workspace_id}");
    let key_id = Uuid::now_v7();
    let raw_key = format!("event-test-key-{workspace_id}");

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
        .bind(format!("event-test-{raw_key}"))
        .bind(hash_key(&raw_key))
        .execute(pool)
        .await
        .expect("api key");

    (raw_key, key_id)
}

impl Harness {
    /// Petición al plano de administración (con auth Bearer).
    async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> Resp {
        self.enviar(method, uri, Some(&self.key), body).await
    }

    /// Petición al plano de invocación `/x/{slug}` (sin auth).
    async fn x(&self, method: Method, slug: &str, body: Option<Value>) -> Resp {
        self.enviar(method, &format!("/x/{slug}"), None, body).await
    }

    async fn enviar(
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
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("cuerpo legible");
        Resp {
            status,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    /// Siembra un puente publicado (v1) con una definición arbitraria
    /// (trigger event/http, emit, blocks…).
    async fn puente(&self, slug: &str, mut definition: Value) {
        definition["trigger"]["path"] = json!(format!("/x/{slug}"));
        let bridge_id = Uuid::now_v7();

        sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
            .bind(bridge_id)
            .bind(self.workspace_id)
            .bind(slug)
            .bind(format!("Puente {slug}"))
            .execute(&self.pool)
            .await
            .expect("bridge");

        sqlx::query(
            "INSERT INTO bridge_versions (id, bridge_id, version, definition) VALUES ($1, $2, 1, $3)",
        )
        .bind(Uuid::now_v7())
        .bind(bridge_id)
        .bind(&definition)
        .execute(&self.pool)
        .await
        .expect("bridge_version");

        sqlx::query("UPDATE bridges SET latest_version = 1 WHERE id = $1")
            .bind(bridge_id)
            .execute(&self.pool)
            .await
            .expect("latest_version");
    }

    /// Número de ejecuciones pendientes del puente `slug`.
    async fn pendientes(&self, slug: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM executions e \
             JOIN bridges b ON b.id = e.bridge_id \
             WHERE b.workspace_id = $1 AND b.slug = $2 AND e.status = 'pending'",
        )
        .bind(self.workspace_id)
        .bind(slug)
        .fetch_one(&self.pool)
        .await
        .expect("conteo de pendientes")
    }
}

/// Bloque JS mínimo que siempre tiene éxito.
const BLOQUE_OK: &str = "export default (input) => ({ ok: true })";

/// Bloque JS que siempre falla.
const BLOQUE_MALO: &str = "export default (input) => { throw new Error('kaput'); }";

// ── fan-out transaccional (task 3.2) ───────────────────────────────────

#[tokio::test]
async fn el_fan_out_encola_una_ejecucion_por_suscriptor_con_el_mismo_evento() {
    let Some(h) = begin().await else { return };

    for slug in ["suscrito-a", "suscrito-b", "suscrito-c"] {
        h.puente(
            slug,
            json!({
                "trigger": { "type": "event", "topic": "pedido.creado" },
                "blocks": [{ "id": "b1", "name": "hacer", "code": BLOQUE_OK,
                             "config": {}, "input": {}, "inputSchema": {}, "outputSchema": {} }]
            }),
        )
        .await;
    }
    // Un puente NO suscrito: no debe recibir el evento.
    h.puente(
        "ajeno",
        json!({
            "trigger": { "type": "event", "topic": "otro.topic" },
            "blocks": [{ "id": "b1", "name": "hacer", "code": BLOQUE_OK,
                         "config": {}, "input": {}, "inputSchema": {}, "outputSchema": {} }]
        }),
    )
    .await;

    let respuesta = h
        .admin(
            Method::POST,
            "/api/v1/events/pedido.creado",
            Some(json!({ "pedidoId": "P-42", "total": 99.5 })),
        )
        .await;
    assert_eq!(respuesta.status, StatusCode::ACCEPTED, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["executions"], json!(3), "{}", respuesta.body);
    assert_eq!(sobre["topic"], json!("pedido.creado"));
    assert!(sobre["eventId"].as_str().is_some());

    // Tres ejecuciones `pending` (una por suscriptor), el ajeno no.
    for slug in ["suscrito-a", "suscrito-b", "suscrito-c"] {
        assert_eq!(h.pendientes(slug).await, 1, "{slug}");
    }
    assert_eq!(h.pendientes("ajeno").await, 0);

    // El mismo eventId para las tres; payload y receivedAt en trigger_data.
    let filas: Vec<(Value, Uuid)> = sqlx::query_as(
        "SELECT trigger_data, bridge_id FROM executions e \
         JOIN bridges b ON b.id = e.bridge_id \
         WHERE b.workspace_id = $1 AND e.trigger_type = 'event'",
    )
    .bind(h.workspace_id)
    .fetch_all(&h.pool)
    .await
    .expect("filas de la cola");
    assert_eq!(filas.len(), 3);
    let event_id = sobre["eventId"].as_str().unwrap();
    for (trigger_data, _) in &filas {
        assert_eq!(trigger_data["eventId"], json!(event_id));
        assert_eq!(trigger_data["topic"], json!("pedido.creado"));
        assert_eq!(trigger_data["payload"]["pedidoId"], json!("P-42"));
        assert!(trigger_data["receivedAt"].as_str().is_some());
    }
    let bridges: std::collections::HashSet<Uuid> = filas.iter().map(|(_, id)| *id).collect();
    assert_eq!(bridges.len(), 3, "una ejecución por puente distinto");

    // El worker corre las tres (en cola, el orden no está garantizado).
    let state = AppState {
        pool: h.pool.clone(),
    };
    let mut corridas = 0;
    while api::worker::procesar_pendiente(&state).await.unwrap() {
        corridas += 1;
    }
    assert_eq!(corridas, 3);
    let estados: Vec<(String,)> = sqlx::query_as(
        "SELECT e.status::text FROM executions e \
         JOIN bridges b ON b.id = e.bridge_id \
         WHERE b.workspace_id = $1 AND e.trigger_type = 'event' ORDER BY b.slug",
    )
    .bind(h.workspace_id)
    .fetch_all(&h.pool)
    .await
    .expect("estados");
    for (estado,) in estados {
        assert_eq!(estado, "succeeded");
    }
}

#[tokio::test]
async fn un_topic_sin_suscriptores_acepta_la_publicacion_sin_retener_el_evento() {
    let Some(h) = begin().await else { return };

    let respuesta = h
        .admin(
            Method::POST,
            "/api/v1/events/nadie.escucha",
            Some(json!({ "dato": 1 })),
        )
        .await;
    assert_eq!(respuesta.status, StatusCode::ACCEPTED, "{}", respuesta.body);
    assert_eq!(respuesta.json()["executions"], json!(0));

    // El evento no se retiene: no hay filas de ningún tipo (sin tabla de
    // eventos por diseño D1).
    let eventos = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM executions WHERE workspace_id = $1 AND trigger_type = 'event'",
    )
    .bind(h.workspace_id)
    .fetch_one(&h.pool)
    .await
    .expect("conteo");
    assert_eq!(eventos, 0);
}

#[tokio::test]
async fn un_topic_invalido_es_400_problem() {
    let Some(h) = begin().await else { return };

    let respuesta = h
        .admin(
            Method::POST,
            "/api/v1/events/Pedido-Creado",
            Some(json!({})),
        )
        .await;
    assert_eq!(
        respuesta.status,
        StatusCode::BAD_REQUEST,
        "{}",
        respuesta.body
    );
    assert!(respuesta.body.contains("topic"), "{}", respuesta.body);
}

// ── emisión al completar (task 3.3) ────────────────────────────────────

#[tokio::test]
async fn emit_on_success_encadena_el_puente_suscriptor() {
    let Some(h) = begin().await else { return };

    // A: http, exitoso, emite en success.
    h.puente(
        "emisor-a",
        json!({
            "trigger": { "type": "http", "method": "POST", "path": "/x/emisor-a" },
            "emit": { "onSuccess": ["pedido.creado"] },
            "blocks": [{ "id": "b1", "name": "hacer", "code": BLOQUE_OK,
                         "config": {}, "input": {}, "inputSchema": {}, "outputSchema": {} }]
        }),
    )
    .await;
    // B: suscrito al topic de A.
    h.puente(
        "suscrito-b",
        json!({
            "trigger": { "type": "event", "topic": "pedido.creado" },
            "blocks": [{ "id": "b1", "name": "hacer", "code": BLOQUE_OK,
                         "config": {}, "input": {}, "inputSchema": {}, "outputSchema": {} }]
        }),
    )
    .await;

    let sobre = h.x(Method::POST, "emisor-a", Some(json!({}))).await;
    assert_eq!(sobre.status, StatusCode::OK, "{}", sobre.body);
    let execution_a = sobre.json()["executionId"].as_str().unwrap().to_string();

    // B quedó encolada por la emisión de A.
    assert_eq!(h.pendientes("suscrito-b").await, 1);
    let (trigger_data,): (Value,) = sqlx::query_as(
        "SELECT trigger_data FROM executions e \
         JOIN bridges b ON b.id = e.bridge_id \
         WHERE b.slug = 'suscrito-b' AND b.workspace_id = $1",
    )
    .bind(h.workspace_id)
    .fetch_one(&h.pool)
    .await
    .expect("fila de B");
    // El payload es el sobre final enriquecido del emisor.
    assert_eq!(trigger_data["payload"]["success"], json!(true));
    assert_eq!(trigger_data["payload"]["data"]["ok"], json!(true));
    assert_eq!(trigger_data["payload"]["bridgeSlug"], json!("emisor-a"));
    assert_eq!(trigger_data["payload"]["executionId"], json!(execution_a));

    // Y B corre con éxito.
    let state = AppState {
        pool: h.pool.clone(),
    };
    assert!(api::worker::procesar_pendiente(&state).await.unwrap());
}

#[tokio::test]
async fn emit_on_failure_solo_emite_cuando_la_ejecucion_falla() {
    let Some(h) = begin().await else { return };

    // A2: http que falla, emite SOLO en failure.
    h.puente(
        "emisor-malo",
        json!({
            "trigger": { "type": "http", "method": "POST", "path": "/x/emisor-malo" },
            "emit": { "onFailure": ["integracion.fallo"] },
            "blocks": [{ "id": "b1", "name": "hacer", "code": BLOQUE_MALO,
                         "config": {}, "input": {}, "inputSchema": {}, "outputSchema": {} }]
        }),
    )
    .await;
    // C suscrito al topic de fallo; D a un topic que A2 no emite nunca.
    for (slug, topic) in [
        ("vigila-fallos", "integracion.fallo"),
        ("ajeno-d", "pedido.creado"),
    ] {
        h.puente(
            slug,
            json!({
                "trigger": { "type": "event", "topic": topic },
                "blocks": [{ "id": "b1", "name": "hacer", "code": BLOQUE_OK,
                             "config": {}, "input": {}, "inputSchema": {}, "outputSchema": {} }]
            }),
        )
        .await;
    }

    let sobre = h.x(Method::POST, "emisor-malo", Some(json!({}))).await;
    assert_eq!(sobre.status, StatusCode::OK, "{}", sobre.body);
    assert_eq!(sobre.json()["success"], json!(false));

    assert_eq!(h.pendientes("vigila-fallos").await, 1, "el fallo emite");
    assert_eq!(h.pendientes("ajeno-d").await, 0, "sin éxito no hay emisión");

    // El payload de fallo trae el código de error y la ejecución emisora.
    let (trigger_data,): (Value,) = sqlx::query_as(
        "SELECT trigger_data FROM executions e \
         JOIN bridges b ON b.id = e.bridge_id WHERE b.slug = 'vigila-fallos'",
    )
    .bind(h.workspace_id)
    .fetch_one(&h.pool)
    .await
    .expect("fila de C");
    assert_eq!(trigger_data["payload"]["success"], json!(false));
    assert_eq!(
        trigger_data["payload"]["error"]["code"],
        json!("BLOCK_FAILED")
    );
    assert_eq!(trigger_data["payload"]["bridgeSlug"], json!("emisor-malo"));
}
