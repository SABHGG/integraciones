//! Prueba end-to-end de MVP3 (task 6.1) contra un Postgres **real**.
//!
//! Un solo hilo de verdad que cruza las fases del hito:
//!
//! 1. **Mask + emit + webhook**: el puente A (http) enmascara la tarjeta del
//!    trigger y de su salida, emite `onSuccess` a `pedido.creado` y entrega
//!    el resultado por webhook firmado — el valor original no aparece en
//!    ninguna tabla ni en la entrega.
//! 2. **Encadenado por evento**: el puente B, suscrito a `pedido.creado`,
//!    queda encolado por la emisión de A y corre con el sobre enriquecido
//!    (`bridgeSlug`/`executionId`) como `$.trigger.payload`.
//! 3. **Dry-run con mock**: el borrador de A se ensaya con un mock y no
//!    deja ninguna ejecución nueva.
//! 4. **Replay**: la ejecución de A se re-ejecuta con su misma versión y
//!    entrada (enmascaradas), con el enlace original ↔ replay visible.
//!
//! Los puentes se siembran por SQL (patrón de `e2e_mvp2`): el receptor local
//! es `http`, que la publicación real rechaza. Corre en la BD dedicada
//! `integraciones_e2e` (worker + entregas reclaman colas globales).

use api::auth::hash_key;
use api::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode, header};
use axum::routing::post as post_route;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

static DB_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// Una entrega capturada por el receptor de mentira.
#[derive(Clone)]
struct Recibida {
    body: String,
}

type Registro = Arc<Mutex<Vec<Recibida>>>;

/// Receptor local de webhooks: graba el body crudo.
async fn receptor() -> (String, Registro) {
    let registro: Registro = Arc::new(Mutex::new(Vec::new()));

    let app = Router::new()
        .route(
            "/hook",
            post_route(
                |State(registro): State<Registro>, request: Request<Body>| async move {
                    let bytes = axum::body::to_bytes(request.into_body(), 1 << 20)
                        .await
                        .expect("cuerpo legible");
                    registro.lock().unwrap().push(Recibida {
                        body: String::from_utf8(bytes.to_vec()).expect("UTF-8"),
                    });
                    StatusCode::OK
                },
            ),
        )
        .with_state(registro.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("puerto");
    let addr = listener.local_addr().expect("dirección");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("receptor");
    });
    (format!("http://{addr}/hook"), registro)
}

struct Mundo {
    app: Router,
    pool: PgPool,
    state: AppState,
    workspace_id: Uuid,
    key: String,
    /// Turno de base de datos, vivo mientras dura el test.
    _turno: MutexGuard<'static, ()>,
}

async fn begin() -> Option<Mundo> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("DATABASE_URL no está definida: se omite el e2e de MVP3");
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
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'e2e3-%'")
        .execute(&pool)
        .await
        .expect("limpieza");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'e2e3%'")
        .execute(&pool)
        .await
        .expect("limpieza de keys");

    let workspace_id = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace e2e MVP3")
        .bind(format!("e2e3-{workspace_id}"))
        .execute(&pool)
        .await
        .expect("workspace");
    let key = format!("e2e3-key-{workspace_id}");
    sqlx::query("INSERT INTO api_keys (id, workspace_id, name, key_hash) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::now_v7())
        .bind(workspace_id)
        .bind(format!("e2e3-{key}"))
        .bind(hash_key(&key))
        .execute(&pool)
        .await
        .expect("api key");

    let app = api::app(AppState { pool: pool.clone() });
    Some(Mundo {
        app,
        state: AppState { pool: pool.clone() },
        pool,
        workspace_id,
        key,
        _turno: turno,
    })
}

impl Mundo {
    /// Petición al plano /x (sin auth) o admin (con auth si `autenticada`).
    async fn send(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        autenticada: bool,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        if autenticada {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {}", self.key));
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
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
            .await
            .expect("cuerpo");
        let text = String::from_utf8(bytes.to_vec()).expect("UTF-8");
        let json = serde_json::from_str(&text).unwrap_or(Value::Null);
        (status, json)
    }

    /// Siembra un puente publicado (v1) cuyo BORRADOR es la misma definición
    /// (para el dry-run). Devuelve el bridge_id.
    async fn sembrar(&self, slug: &str, definition: Value) -> Uuid {
        let bridge_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO bridges (id, workspace_id, slug, name, draft) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(bridge_id)
        .bind(self.workspace_id)
        .bind(slug)
        .bind(format!("Puente {slug}"))
        .bind(&definition)
        .execute(&self.pool)
        .await
        .expect("bridge");
        // Secreto HMAC del webhook si la definición lo declara (la entrega
        // firmada falla cerrado sin él).
        if definition.get("webhook").is_some() {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).expect("azar");
            let secreto = hex::encode(bytes);
            sqlx::query("UPDATE bridges SET webhook_secret = $2 WHERE id = $1")
                .bind(bridge_id)
                .bind(api::vault::encrypt(&secreto))
                .execute(&self.pool)
                .await
                .expect("secreto");
        }
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
        bridge_id
    }

    /// Ejecuciones totales del workspace (para verificar que el dry-run no
    /// deja rastro).
    async fn total_ejecuciones(&self) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM executions WHERE workspace_id = $1")
            .bind(self.workspace_id)
            .fetch_one(&self.pool)
            .await
            .expect("conteo")
    }
}

#[tokio::test]
async fn e2e_mvp3_mask_emit_evento_dry_run_y_replay() {
    let Some(mundo) = begin().await else { return };
    let (url_hook, registro) = receptor().await;

    // Puente A: http + mask (trigger y salida) + emit.onSuccess + webhook.
    // El bloque hace eco del body del trigger: la tarjeta viaja al output y
    // al webhook, donde debe llegar enmascarada.
    let definicion_a = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/captura" },
        "webhook": { "url": url_hook },
        "emit": { "onSuccess": ["pedido.creado"] },
        "mask": ["$.trigger.body.tarjeta", "$.capturar.recibido.tarjeta"],
        "blocks": [{
            "id": "b1",
            "name": "capturar",
            "code": "export default function(input) { return { recibido: input.cuerpo }; }",
            "input": { "cuerpo": "$.trigger.body" },
            "config": {},
            "outputSchema": {}
        }]
    });
    mundo.sembrar("captura", definicion_a).await;

    // Puente B: suscrito a pedido.creado, consume el payload del sobre.
    mundo
        .sembrar(
            "procesa",
            json!({
                "trigger": { "type": "event", "topic": "pedido.creado" },
                "blocks": [{
                    "id": "b1",
                    "name": "procesar",
                    "code": "export default function(input) { return { total: input.total, emisor: input.emisor }; }",
                    "input": {
                        "total": "$.trigger.payload.data.recibido.total",
                        "emisor": "$.trigger.payload.bridgeSlug"
                    },
                    "config": {},
                    "outputSchema": {}
                }]
            }),
        )
        .await;

    let ejecuciones_antes = mundo.total_ejecuciones().await;

    // ── 1. Ejecución de A con tarjeta sensible ───────────────────────────
    let (status, sobre) = mundo
        .send(
            "POST",
            "/x/captura",
            Some(json!({ "total": 7, "tarjeta": "4242-1111-1111-1111" })),
            false,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{sobre}");
    assert_eq!(sobre["success"], json!(true));
    // Mask visible ya en el sobre de respuesta.
    assert_eq!(sobre["data"]["recibido"]["tarjeta"], json!("***"));
    assert_eq!(sobre["data"]["recibido"]["total"], json!(7));
    let execution_a = sobre["executionId"]
        .as_str()
        .expect("executionId")
        .to_string();

    // Irreversibilidad: el valor real no vive en ninguna tabla.
    let con_valor = sqlx::query_scalar::<_, i64>(
        "SELECT (SELECT count(*) FROM executions WHERE workspace_id = $1 AND trigger_data::text LIKE '%4242-1111%') \
         + (SELECT count(*) FROM execution_steps WHERE output::text LIKE '%4242-1111%')",
    )
    .bind(mundo.workspace_id)
    .fetch_one(&mundo.pool)
    .await
    .expect("conteo de fugas");
    assert_eq!(con_valor, 0, "el número de tarjeta sobrevivió en el store");

    // ── 2. Encadenado por evento: B encolado por la emisión de A ─────────
    let (pendientes_b,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM executions e \
         JOIN bridges b ON b.id = e.bridge_id \
         WHERE b.slug = 'procesa' AND e.status = 'pending'",
    )
    .fetch_one(&mundo.pool)
    .await
    .expect("pendientes de B");
    assert_eq!(pendientes_b, 1, "la emisión de A encoló a B");
    let (trigger_b,): (Value,) = sqlx::query_as(
        "SELECT trigger_data FROM executions e \
         JOIN bridges b ON b.id = e.bridge_id WHERE b.slug = 'procesa'",
    )
    .fetch_one(&mundo.pool)
    .await
    .expect("trigger de B");
    assert_eq!(trigger_b["payload"]["bridgeSlug"], json!("captura"));
    assert_eq!(trigger_b["payload"]["executionId"], json!(execution_a));
    // El payload viaja enmascarado (el sobre se persistió enmascarado).
    assert_eq!(
        trigger_b["payload"]["data"]["recibido"]["tarjeta"],
        json!("***")
    );

    // B corre y tiene éxito con el total del pedido.
    assert!(
        api::worker::procesar_pendiente(&mundo.state)
            .await
            .expect("worker")
    );
    let (estado_b, salida_b): (String, Value) = sqlx::query_as(
        "SELECT e.status::text, (SELECT output FROM execution_steps s WHERE s.execution_id = e.id AND s.seq = 1) \
         FROM executions e JOIN bridges b ON b.id = e.bridge_id WHERE b.slug = 'procesa'",
    )
    .fetch_one(&mundo.pool)
    .await
    .expect("estado de B");
    assert_eq!(estado_b, "succeeded");
    assert_eq!(salida_b["total"], json!(7));
    assert_eq!(salida_b["emisor"], json!("captura"));

    // ── 3. Mask visible en el trace y en la entrega webhook ──────────────
    let (status, trace) = mundo
        .send(
            "GET",
            &format!("/api/v1/executions/{execution_a}/trace"),
            None,
            true,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{trace}");
    assert_eq!(
        trace["steps"][0]["output"]["recibido"]["tarjeta"],
        json!("***"),
        "el trace entrega el IO enmascarado"
    );
    assert_eq!(trace["steps"][0]["output"]["recibido"]["total"], json!(7));

    // El webhook de A entrega el sobre persistido (enmascarado).
    assert!(
        api::webhooks::procesar_una_entrega(&mundo.state)
            .await
            .expect("entrega webhook"),
        "había una entrega pendiente"
    );
    {
        let entregas = registro.lock().unwrap();
        assert_eq!(entregas.len(), 1);
        let cuerpo_webhook: Value =
            serde_json::from_str(&entregas[0].body).expect("JSON del webhook");
        assert_eq!(cuerpo_webhook["success"], json!(true));
        assert_eq!(
            cuerpo_webhook["data"]["recibido"]["tarjeta"],
            json!("***"),
            "el webhook recibe el dato enmascarado"
        );
    } // el guard se suelta antes de los siguientes awaits

    // ── 4. Dry-run del borrador con mock: cero ejecuciones nuevas ────────
    let ejecuciones_pre_dry = mundo.total_ejecuciones().await;
    let bridge_a: Uuid = sqlx::query_scalar("SELECT id FROM bridges WHERE slug = 'captura'")
        .fetch_one(&mundo.pool)
        .await
        .expect("puente A");
    // El dry-run exige el mismo estándar que la publicación: bloque de
    // biblioteca real y borrador sin webhook http (el receptor local no es
    // https). El bloque se mockea, así que su código no llega a correr.
    let (status, bloque) = mundo
        .send(
            "POST",
            "/api/v1/blocks",
            Some(json!({ "slug": "eco-e2e3", "name": "eco", "blockType": "custom_logic" })),
            true,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{bloque}");
    let block_id = bloque["id"].as_str().expect("block id").to_string();
    let (status, version) = mundo
        .send(
            "POST",
            &format!("/api/v1/blocks/{block_id}/versions"),
            Some(json!({
                "code": "export default (input) => input;",
                "inputSchema": {},
                "outputSchema": {}
            })),
            true,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{version}");
    let block_version_id = version["id"].as_str().expect("version id").to_string();
    let draft_valido = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/captura" },
        "blocks": [{
            "id": "b1",
            "name": "capturar",
            "blockId": block_id,
            "blockVersionId": block_version_id,
            "input": { "cuerpo": "$.trigger.body" },
            "config": {}
        }]
    });
    sqlx::query("UPDATE bridges SET draft = $2 WHERE id = $1")
        .bind(bridge_a)
        .bind(&draft_valido)
        .execute(&mundo.pool)
        .await
        .expect("borrador válido");
    let (status, ensayo) = mundo
        .send(
            "POST",
            &format!("/api/v1/bridges/{bridge_a}/dry-run"),
            Some(json!({
                "triggerData": { "total": 999, "tarjeta": "0000-0000-0000-0000" },
                "mocks": { "capturar": { "output": { "recibido": { "fingido": true } } } }
            })),
            true,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{ensayo}");
    assert_eq!(ensayo["status"], json!("succeeded"));
    assert_eq!(
        ensayo["steps"][0]["output"],
        json!({ "recibido": { "fingido": true } }),
        "el mock alimenta el contexto sin invocar el bloque"
    );
    assert_eq!(
        mundo.total_ejecuciones().await,
        ejecuciones_pre_dry,
        "el dry-run no deja rastro de ejecución"
    );

    // ── 5. Replay de A: misma versión y entrada, original intacto ────────
    let (status, ack) = mundo
        .send(
            "POST",
            &format!("/api/v1/executions/{execution_a}/replay"),
            None,
            true,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{ack}");
    let replay_id = ack["executionId"]
        .as_str()
        .expect("executionId")
        .to_string();
    assert_eq!(ack["replayOf"], json!(execution_a));

    assert!(
        api::worker::procesar_pendiente(&mundo.state)
            .await
            .expect("worker")
    );
    let (version_original, version_replay): (Uuid, Uuid) = sqlx::query_as(
        "SELECT (SELECT bridge_version_id FROM executions WHERE id = $1), \
                (SELECT bridge_version_id FROM executions WHERE id = $2)",
    )
    .bind(Uuid::parse_str(&execution_a).expect("uuid"))
    .bind(Uuid::parse_str(&replay_id).expect("uuid"))
    .fetch_one(&mundo.pool)
    .await
    .expect("versiones");
    assert_eq!(
        version_original, version_replay,
        "el replay usa la versión congelada"
    );

    // El detalle de la original lista el replay; el estado original no cambió.
    let (status, detalle) = mundo
        .send(
            "GET",
            &format!("/api/v1/executions/{execution_a}"),
            None,
            true,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{detalle}");
    assert_eq!(detalle["status"], json!("succeeded"));
    let replays = detalle["replays"].as_array().expect("replays");
    assert_eq!(replays.len(), 1);
    assert_eq!(replays[0]["id"], json!(replay_id));
    assert_eq!(replays[0]["status"], json!("succeeded"));

    // ── 6. Fan-out directo por API (además del emit) ─────────────────────
    let (status, publicacion) = mundo
        .send(
            "POST",
            "/api/v1/events/pedido.creado",
            Some(json!({ "externo": true })),
            true,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{publicacion}");
    assert_eq!(publicacion["executions"], json!(1));

    // Higiene: drena la cola completa Y las entregas webhook pendientes
    // (el replay de A re-emite la entrega) para no contaminar al binario de
    // e2e_mvp2, que comparte esta BD dedicada.
    while api::worker::procesar_pendiente(&mundo.state)
        .await
        .expect("worker")
    {}
    while api::webhooks::procesar_una_entrega(&mundo.state)
        .await
        .expect("entregas")
    {}
    // 5: A, B (emit de A), replay de A, B (el replay re-emite onSuccess) y
    // B (fan-out directo por API).
    assert_eq!(mundo.total_ejecuciones().await, ejecuciones_antes + 5);
}
