//! Prueba end-to-end de MVP2 (task 6.1) contra un Postgres **real**.
//!
//! Dos escenarios encadenados de verdad, con las piezas reales (plano /x,
//! worker de la cola, job de entregas webhook, scheduler cron):
//!
//! 1. **Puente condicional async**: un puente con un `switch` de dos rutas y
//!    webhook de resultado se dispara con `Prefer: respond-async` → 202 →
//!    polling del resultado → trace con la ruta no tomada `skipped` → entrega
//!    firmada (HMAC-SHA256) recibida en un servidor local.
//! 2. **Puente cron con solape simulado**: el scheduler pre-encola la próxima
//!    ocurrencia y, con la ejecución viva, registra el solape como
//!    `missed_triggers(overlap)`.
//!
//! Los snapshots se siembran por SQL (patrón de `webhooks_api`): el receptor
//! local es `http`, que la publicación rechaza — la validación https ya tiene
//! sus tests en `bridges_api`. Corre en la BD dedicada `integraciones_e2e`
//! porque el worker y el job de entregas reclaman colas globales.

use api::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode, header};
use axum::routing::post as post_route;
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

static DB_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

#[derive(Clone)]
struct Recibida {
    body: String,
    signature: String,
}

type Registro = Arc<Mutex<Vec<Recibida>>>;

async fn receptor() -> (String, Registro) {
    let registro: Registro = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route(
            "/hook",
            post_route(
                |State(registro): State<Registro>, request: Request<Body>| async move {
                    let signature = header_value(&request, "x-integraciones-signature");
                    let bytes = axum::body::to_bytes(request.into_body(), 1 << 20)
                        .await
                        .expect("cuerpo legible");
                    registro.lock().unwrap().push(Recibida {
                        body: String::from_utf8(bytes.to_vec()).expect("UTF-8"),
                        signature,
                    });
                    StatusCode::OK
                },
            ),
        )
        .with_state(registro.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("puerto efímero");
    let addr = listener.local_addr().expect("dirección");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("receptor");
    });
    (format!("http://{addr}/hook"), registro)
}

fn header_value(request: &Request<Body>, name: &str) -> String {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

struct Mundo {
    app: Router,
    pool: PgPool,
    state: AppState,
    workspace_id: Uuid,
    _turno: MutexGuard<'static, ()>,
}

async fn begin() -> Option<Mundo> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("DATABASE_URL no está definida: se omite el e2e de MVP2");
            return None;
        }
    };
    let url = url
        .rsplit_once('/')
        .map(|(base, _db)| format!("{base}/integraciones_e2e"))
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

    let turno = DB_LOCK.lock().await;
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'e2e2-%'")
        .execute(&pool)
        .await
        .expect("limpieza");

    let workspace_id = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace e2e MVP2")
        .bind(format!("e2e2-{workspace_id}"))
        .execute(&pool)
        .await
        .expect("workspace");

    let app = api::app(AppState { pool: pool.clone() });
    Some(Mundo {
        app,
        state: AppState { pool: pool.clone() },
        pool,
        workspace_id,
        _turno: turno,
    })
}

impl Mundo {
    async fn send(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        prefer: Option<&str>,
    ) -> (StatusCode, Value, String) {
        let mut builder = Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        if let Some(prefer) = prefer {
            builder = builder.header("prefer", prefer);
        }
        let body_bytes = body
            .map(|v| Body::from(v.to_string()))
            .unwrap_or(Body::empty());
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body_bytes).expect("petición"))
            .await
            .expect("respuesta");
        let status = response.status();
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
            .await
            .expect("cuerpo");
        let text = String::from_utf8(bytes.to_vec()).expect("UTF-8");
        let json = serde_json::from_str(&text).unwrap_or(Value::Null);
        (status, json, location)
    }

    /// Siembra un puente publicado: `switch` con dos rutas + webhook, o cron.
    /// Devuelve (bridge_id, secreto webhook si lo lleva).
    async fn sembrar(
        &self,
        slug: &str,
        definition: Value,
        con_webhook: bool,
    ) -> (Uuid, Option<String>) {
        let bridge_id = Uuid::now_v7();
        sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
            .bind(bridge_id)
            .bind(self.workspace_id)
            .bind(slug)
            .bind(format!("Puente {slug}"))
            .execute(&self.pool)
            .await
            .expect("bridge");
        let secreto = if con_webhook {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).expect("azar");
            let secreto = hex::encode(bytes);
            sqlx::query("UPDATE bridges SET webhook_secret = $2 WHERE id = $1")
                .bind(bridge_id)
                .bind(api::vault::encrypt(&secreto))
                .execute(&self.pool)
                .await
                .expect("secreto");
            Some(secreto)
        } else {
            None
        };
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
        (bridge_id, secreto)
    }
}

fn firma(secreto: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secreto.as_bytes()).expect("clave");
    mac.update(body.as_bytes());
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

fn at(instante: &str) -> chrono::DateTime<Utc> {
    chrono::NaiveDateTime::parse_from_str(instante, "%Y-%m-%dT%H:%M:%S")
        .expect("instante")
        .and_utc()
}

#[tokio::test]
async fn e2e_puente_condicional_async_con_polling_y_webhook_firmado() {
    let Some(mundo) = begin().await else { return };
    let (url_hook, registro) = receptor().await;

    // Puente: switch con dos rutas (vip ≥ 10 → precio-vip; default → precio).
    let definition = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/checkout" },
        "webhook": { "url": url_hook },
        "blocks": [
            {
                "id": "b-switch",
                "name": "enrutador",
                "code": null,
                "input": {},
                "config": {
                    "routes": [
                        { "when": { "gte": ["$.trigger.body.total", 10] }, "to": "b-vip" },
                        { "when": { "exists": "$.trigger.body.total" }, "to": "b-normal" }
                    ],
                    "default": "b-normal"
                },
                "outputSchema": {}
            },
            {
                "id": "b-vip",
                "name": "vip",
                "code": "export default function(input, ctx) { return { plan: \"vip\", total: input.total }; }",
                "input": { "total": "$.trigger.body.total" },
                "config": {},
                "outputSchema": {}
            },
            {
                "id": "b-normal",
                "name": "normal",
                "code": "export default function(input, ctx) { return { plan: \"normal\", total: input.total }; }",
                "input": { "total": "$.trigger.body.total" },
                "config": {},
                "outputSchema": {}
            }
        ]
    });
    let (bridge_id, secreto) = mundo.sembrar("checkout", definition, true).await;
    let secreto = secreto.expect("webhook con secreto");

    // 1. Disparo async: 202 + Location; la fila nace pending.
    let (status, ack, location) = mundo
        .send(
            "POST",
            "/x/checkout",
            Some(json!({ "total": 25 })),
            Some("respond-async"),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{ack}");
    let execution_id = ack["executionId"]
        .as_str()
        .expect("executionId")
        .to_string();
    assert_eq!(location, format!("/x/checkout/executions/{execution_id}"));

    let (estado,): (String,) = sqlx::query_as("SELECT status FROM executions WHERE id = $1")
        .bind(execution_id.parse::<Uuid>().expect("uuid"))
        .fetch_one(&mundo.pool)
        .await
        .expect("ejecución");
    assert_eq!(estado, "pending");

    // 2. Polling en curso: pending sin data.
    let (status, poll, _) = mundo
        .send(
            "GET",
            &format!("/x/checkout/executions/{execution_id}"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(poll["status"], json!("pending"));
    assert!(
        poll.get("data").is_none(),
        "sin data mientras corre: {poll}"
    );

    // 3. El worker corre la cadena: la ruta vip (total 25 ≥ 10) ejecuta y la
    //    normal queda skipped en su seq.
    assert!(
        api::worker::procesar_pendiente(&mundo.state)
            .await
            .expect("worker"),
        "la fila async era reclamable"
    );

    // 4. Polling final: sobre idéntico al síncrono con la salida de la ruta.
    let (status, sobre, _) = mundo
        .send(
            "GET",
            &format!("/x/checkout/executions/{execution_id}"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        sobre,
        json!({ "success": true, "data": { "plan": "vip", "total": 25 }, "executionId": execution_id })
    );

    // 5. El trace muestra el switch y la ruta no tomada como skipped
    //    (consultado directamente en execution_steps; el endpoint del trace
    //    ya tiene sus tests en executions_api).
    let pasos: Vec<(String, String)> = sqlx::query_as(
        "SELECT block_name, status FROM execution_steps \
         WHERE execution_id = $1 ORDER BY seq",
    )
    .bind(execution_id.parse::<Uuid>().expect("uuid"))
    .fetch_all(&mundo.pool)
    .await
    .expect("pasos");
    assert_eq!(
        pasos,
        vec![
            ("enrutador".to_string(), "succeeded".to_string()),
            ("vip".to_string(), "succeeded".to_string()),
            ("normal".to_string(), "skipped".to_string()),
        ]
    );

    // 6. La entrega webhook queda programada y el job la envía firmada.
    let (d_estado,): (String,) =
        sqlx::query_as("SELECT status FROM webhook_deliveries WHERE execution_id = $1")
            .bind(execution_id.parse::<Uuid>().expect("uuid"))
            .fetch_one(&mundo.pool)
            .await
            .expect("entrega");
    assert_eq!(d_estado, "pending");

    assert!(
        api::webhooks::procesar_una_entrega(&mundo.state)
            .await
            .expect("entrega")
    );

    let recibidas = registro.lock().unwrap().clone();
    assert_eq!(recibidas.len(), 1);
    let recibida = &recibidas[0];
    assert_eq!(
        recibida.signature,
        firma(&secreto, &recibida.body),
        "la firma verifica con el secreto del puente"
    );
    let payload: Value = serde_json::from_str(&recibida.body).expect("payload");
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"], json!({ "plan": "vip", "total": 25 }));
    assert_eq!(payload["bridgeSlug"], json!("checkout"));
    assert_eq!(payload["status"], json!("succeeded"));

    let (d_estado,): (String,) =
        sqlx::query_as("SELECT status FROM webhook_deliveries WHERE execution_id = $1")
            .bind(execution_id.parse::<Uuid>().expect("uuid"))
            .fetch_one(&mundo.pool)
            .await
            .expect("entrega");
    assert_eq!(d_estado, "delivered");
    let _ = bridge_id;

    // Higiene: drena la cola global y las entregas pendientes para no
    // contaminar al test cron (y a otros binarios que comparten la BD).
    while api::worker::procesar_pendiente(&mundo.state)
        .await
        .expect("worker")
    {}
    while api::webhooks::procesar_una_entrega(&mundo.state)
        .await
        .expect("entregas")
    {}
}

#[tokio::test]
async fn e2e_puente_cron_con_solape_simulado() {
    let Some(mundo) = begin().await else { return };

    let definition = json!({
        "trigger": { "type": "cron", "schedule": "* * * * *", "timezone": "UTC" },
        "blocks": [{
            "id": "b1",
            "name": "eco",
            "code": "export default function(input, ctx) { return { ok: true }; }",
            "input": {},
            "config": {},
            "outputSchema": {}
        }]
    });
    let (bridge_id, _) = mundo.sembrar("cada-minuto-e2e", definition, false).await;

    // El scheduler pre-encola la 09:05 con run_at programado.
    api::scheduler::tick(&mundo.state, at("2026-03-04T09:04:30"))
        .await
        .expect("tick");
    let filas: Vec<(Value, String)> = sqlx::query_as(
        "SELECT trigger_data->'scheduled_at', status FROM executions \
         WHERE bridge_id = $1 AND trigger_type = 'cron' ORDER BY 1",
    )
    .bind(bridge_id)
    .fetch_all(&mundo.pool)
    .await
    .expect("filas cron");
    assert_eq!(
        filas,
        vec![(json!("2026-03-04T09:05:00+00:00"), "pending".to_string())]
    );

    // La ejecución queda `pending` viva (un puente lento: nadie la corre).

    // A las 09:07 la siguiente ocurrencia venció con la 09:05 aún viva:
    // el scheduler la salta y la registra como overlap (política fija skip).
    api::scheduler::tick(&mundo.state, at("2026-03-04T09:07:30"))
        .await
        .expect("tick solape");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM executions \
             WHERE bridge_id = $1 AND trigger_type = 'cron' AND status IN ('pending','running')"
        )
        .bind(bridge_id)
        .fetch_one(&mundo.pool)
        .await
        .expect("vivas"),
        1,
        "sigue viva la original, sin acumular"
    );
    let (reason, scheduled): (String, Value) = sqlx::query_as(
        "SELECT reason, to_jsonb(scheduled_at) FROM missed_triggers WHERE bridge_id = $1",
    )
    .bind(bridge_id)
    .fetch_one(&mundo.pool)
    .await
    .expect("missed");
    assert_eq!(reason, "overlap");
    assert_eq!(scheduled, json!("2026-03-04T09:06:00+00:00"));

    // Higiene: las ocurrencias cron pre-encoladas tienen run_at ya pasado y
    // son reclamables por la cola global: drenarlas para que el otro test de
    // este binario (y los e2e siguientes) reclamen SOLO sus filas.
    while api::worker::procesar_pendiente(&mundo.state)
        .await
        .expect("worker")
    {}
    while api::webhooks::procesar_una_entrega(&mundo.state)
        .await
        .expect("entregas")
    {}
}
