//! Tests de la entrega de resultado por webhook firmado (MVP2, tasks 4.2-4.5).
//!
//! Montan un servidor HTTP local que hace de receptor: verifica la firma
//! HMAC-SHA256 del body crudo con el secreto expuesto por el plano admin,
//! deduplica por `X-Integraciones-Delivery` y puede fallar N veces para
//! ejercitar los reintentos con backoff.
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
use axum::extract::State;
use axum::http::{Request, StatusCode, header};
use axum::routing::post as post_route;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgPool;
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

/// Secretos sembrados por puente (en claro, para verificar firmas en el test).
static SECRETS: std::sync::LazyLock<Mutex<std::collections::HashMap<Uuid, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Los tests comparten base de datos: se serializan.
static DB_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// Una petición capturada por el receptor de mentira.
#[derive(Clone)]
struct Recibida {
    body: String,
    signature: String,
    timestamp: String,
    delivery: String,
}

type Registro = Arc<Mutex<Vec<Recibida>>>;

/// Número de respuestas 500 que el receptor devuelve antes de responder 200.
type FallosAntesDeExito = Arc<Mutex<usize>>;

/// Servidor receptor: graba todo y falla N veces si se le pide.
async fn receptor() -> (String, Registro, FallosAntesDeExito) {
    let registro: Registro = Arc::new(Mutex::new(Vec::new()));
    let fallos: FallosAntesDeExito = Arc::new(Mutex::new(0));

    let app = Router::new()
        .route(
            "/hook",
            post_route(
                |State((registro, fallos)): State<(Registro, FallosAntesDeExito)>,
                 request: Request<Body>| async move {
                    let signature = header_value(&request, "x-integraciones-signature");
                    let timestamp = header_value(&request, "x-integraciones-timestamp");
                    let delivery = header_value(&request, "x-integraciones-delivery");
                    let bytes = axum::body::to_bytes(request.into_body(), 1 << 20)
                        .await
                        .expect("cuerpo legible");
                    registro.lock().unwrap().push(Recibida {
                        body: String::from_utf8(bytes.to_vec()).expect("UTF-8"),
                        signature,
                        timestamp,
                        delivery,
                    });
                    let quedan = {
                        let mut guard = fallos.lock().unwrap();
                        if *guard > 0 {
                            *guard -= 1;
                            true
                        } else {
                            false
                        }
                    };
                    if quedan {
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR
                    } else {
                        axum::http::StatusCode::OK
                    }
                },
            ),
        )
        .with_state((registro.clone(), fallos.clone()));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("puerto efímero");
    let addr = listener.local_addr().expect("dirección local");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("receptor de mentira");
    });

    (format!("http://{addr}/hook"), registro, fallos)
}

fn header_value(request: &Request<Body>, name: &str) -> String {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

struct Harness {
    app: Router,
    pool: PgPool,
    workspace_id: Uuid,
    _turno: MutexGuard<'static, ()>,
}

async fn begin() -> Option<Harness> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!(
                "DATABASE_URL no está definida: se omiten los tests de integración de webhooks"
            );
            return None;
        }
    };

    let pool = api::connect(&url)
        .await
        .expect("conexión a Postgres (¿corriste ./scripts/dev-postgres.sh?)");
    api::migrate(&pool).await.expect("migraciones aplicadas");

    let turno = DB_LOCK.lock().await;

    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'hook-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces previos");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'hook-test%'")
        .execute(&pool)
        .await
        .expect("limpieza de keys previas");

    let workspace_id = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace webhook")
        .bind(format!("hook-test-{workspace_id}"))
        .execute(&pool)
        .await
        .expect("workspace");
    sqlx::query("INSERT INTO api_keys (id, workspace_id, name, key_hash) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::now_v7())
        .bind(workspace_id)
        .bind(format!("hook-test-key-{workspace_id}"))
        .bind(hash_key(&format!("hook-test-key-{workspace_id}")))
        .execute(&pool)
        .await
        .expect("api key");

    let app = api::app(AppState { pool: pool.clone() });

    Some(Harness {
        app,
        pool,
        workspace_id,
        _turno: turno,
    })
}

impl Harness {
    /// Siembra un puente publicado con snapshot completo (incluye `webhook`),
    /// evitando toda la maquinaria de borrador/publicación: los tests de
    /// entrega se centran en la cola de entregas y la firma.
    async fn bridge_con_webhook(&self, slug: &str, webhook_url: &str, fallar: bool) -> Uuid {
        let bridge_id = Uuid::now_v7();
        let code = if fallar {
            "export default function(input, ctx) { throw new Error(\"boom-hook\"); }"
        } else {
            "export default function(input, ctx) { return { total: input.total * 2 }; }"
        };
        let definition = json!({
            "trigger": { "type": "http", "method": "POST", "path": format!("/{slug}") },
            "webhook": { "url": webhook_url },
            "blocks": [{
                "id": "b1",
                "name": "duplicar",
                "code": code,
                "input": { "total": "$.trigger.body.total" },
                "config": {},
                "outputSchema": {}
            }]
        });
        sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
            .bind(bridge_id)
            .bind(self.workspace_id)
            .bind(slug)
            .bind(format!("Puente {slug}"))
            .execute(&self.pool)
            .await
            .expect("bridge");
        // Secreto del webhook, como lo deja la publicación (cifrado en reposo).
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("azar");
        let secreto = hex::encode(bytes);
        sqlx::query("UPDATE bridges SET webhook_secret = $2 WHERE id = $1")
            .bind(bridge_id)
            .bind(api::vault::encrypt(&secreto))
            .execute(&self.pool)
            .await
            .expect("secreto");
        SECRETS.lock().unwrap().insert(bridge_id, secreto);
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

    /// Secreto en claro del puente (lo que siembra `bridge_con_webhook`).
    fn secreto(&self, bridge_id: Uuid) -> String {
        SECRETS
            .lock()
            .unwrap()
            .get(&bridge_id)
            .expect("secreto del puente sembrado")
            .clone()
    }

    /// Invoca el puente (síncrono) y devuelve el executionId.
    async fn invocar(&self, slug: &str, body: Value) -> String {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/x/{slug}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("petición válida"),
            )
            .await
            .expect("respuesta");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("cuerpo");
        let sobre: Value = serde_json::from_slice(&bytes).expect("sobre JSON");
        sobre["executionId"]
            .as_str()
            .expect("executionId")
            .to_string()
    }
}

/// Verifica `sha256=<hex>` contra el body con el secreto dado.
fn firma_valida(secreto: &str, body: &str, signature: &str) -> bool {
    let mut mac = Hmac::<Sha256>::new_from_slice(secreto.as_bytes()).expect("clave");
    mac.update(body.as_bytes());
    let esperada = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    signature == esperada
}

#[tokio::test]
async fn la_entrega_firmada_llega_al_receptor_con_el_sobre_y_los_metadatos() {
    let Some(h) = begin().await else { return };
    let (url, registro, _fallos) = receptor().await;

    h.bridge_con_webhook("hook-eco", &url, false).await;
    let execution_id = h.invocar("hook-eco", json!({ "total": 21 })).await;

    // 4.2: la fila de entrega existe `pending` y la URL quedó copiada.
    let (estado, webhook_url): (String, Option<String>) = sqlx::query_as(
        "SELECT d.status, e.webhook_url FROM webhook_deliveries d \
         JOIN executions e ON e.id = d.execution_id \
         JOIN bridges b ON b.id = e.bridge_id \
         WHERE b.slug = 'hook-eco'",
    )
    .fetch_one(&h.pool)
    .await
    .expect("fila de entrega");
    assert_eq!(estado, "pending");
    assert_eq!(webhook_url.as_deref(), Some(url.as_str()));

    // 4.3: el job entrega y verifica firma en el receptor.
    let state = AppState {
        pool: h.pool.clone(),
    };
    assert!(
        api::webhooks::procesar_una_entrega(&state)
            .await
            .expect("entrega"),
        "había una entrega pendiente"
    );

    let recibidas = registro.lock().unwrap().clone();
    assert_eq!(recibidas.len(), 1, "una sola petición");
    let recibida = &recibidas[0];

    let secreto = h.secreto(
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM bridges WHERE slug = 'hook-eco'")
            .fetch_one(&h.pool)
            .await
            .expect("bridge id"),
    );

    assert!(
        firma_valida(&secreto, &recibida.body, &recibida.signature),
        "la firma HMAC del body crudo verifica con el secreto del puente"
    );
    assert!(
        !recibida.timestamp.is_empty(),
        "X-Integraciones-Timestamp presente"
    );
    assert!(
        Uuid::parse_str(&recibida.delivery).is_ok(),
        "X-Integraciones-Delivery es un UUID estable: {}",
        recibida.delivery
    );

    let payload: Value = serde_json::from_str(&recibida.body).expect("payload JSON");
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"], json!({ "total": 42 }));
    assert_eq!(payload["executionId"], json!(execution_id));
    assert_eq!(payload["bridgeSlug"], json!("hook-eco"));
    assert_eq!(payload["status"], json!("succeeded"));
    assert!(
        payload["finishedAt"].as_str().is_some(),
        "finishedAt presente"
    );

    // La entrega queda `delivered` con su 200.
    let (estado, codigo): (String, Option<i32>) = sqlx::query_as(
        "SELECT d.status, d.response_status FROM webhook_deliveries d \
         JOIN executions e ON e.id = d.execution_id \
         JOIN bridges b ON b.id = e.bridge_id \
         WHERE b.slug = 'hook-eco'",
    )
    .fetch_one(&h.pool)
    .await
    .expect("entrega");
    assert_eq!(estado, "delivered");
    assert_eq!(codigo, Some(200));
}

#[tokio::test]
async fn los_reintentos_aplican_backoff_y_agotan_en_failed() {
    let Some(h) = begin().await else { return };
    let (url, registro, fallos) = receptor().await;
    *fallos.lock().unwrap() = 100; // el receptor falla siempre

    h.bridge_con_webhook("hook-duro", &url, false).await;
    h.invocar("hook-duro", json!({ "total": 1 })).await;

    let state = AppState {
        pool: h.pool.clone(),
    };
    let delivery_id: Uuid = sqlx::query_scalar(
        "SELECT d.id FROM webhook_deliveries d \
         JOIN executions e ON e.id = d.execution_id \
         JOIN bridges b ON b.id = e.bridge_id WHERE b.slug = 'hook-duro'",
    )
    .fetch_one(&h.pool)
    .await
    .expect("entrega");

    // Intento 1: el 500 queda `pending` con next_retry_at a ~30 s.
    assert!(
        api::webhooks::procesar_una_entrega(&state)
            .await
            .expect("intento 1")
    );
    let (attempts, estado, retry): (i32, String, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
        "SELECT attempts, status, next_retry_at FROM webhook_deliveries WHERE id = $1",
    )
    .bind(delivery_id)
    .fetch_one(&h.pool)
    .await
    .expect("entrega");
    assert_eq!((attempts, estado.as_str()), (1, "pending"));
    let espera = (retry - chrono::Utc::now()).num_seconds();
    assert!(
        (20..=35).contains(&espera),
        "backoff del primer reintento ~30 s (espera {espera} s)"
    );

    // Intentos 2..5: se simula el paso del tiempo acercando next_retry_at.
    for esperado in 2..=5 {
        sqlx::query(
            "UPDATE webhook_deliveries SET next_retry_at = now() - interval '1 s' WHERE id = $1",
        )
        .bind(delivery_id)
        .execute(&h.pool)
        .await
        .expect("acercar el reintento");
        assert!(
            api::webhooks::procesar_una_entrega(&state)
                .await
                .expect("reintento"),
            "intento {esperado} reclamable"
        );
        let (attempts, estado): (i32, String) =
            sqlx::query_as("SELECT attempts, status FROM webhook_deliveries WHERE id = $1")
                .bind(delivery_id)
                .fetch_one(&h.pool)
                .await
                .expect("entrega");
        assert_eq!(attempts, esperado);
        if esperado < 5 {
            assert_eq!(estado, "pending");
        } else {
            assert_eq!(estado, "failed", "agotados los 5 intentos");
        }
    }

    // Sin reintentos extra: la entrega `failed` ya no es reclamable.
    sqlx::query(
        "UPDATE webhook_deliveries SET next_retry_at = now() - interval '1 s' WHERE id = $1",
    )
    .bind(delivery_id)
    .execute(&h.pool)
    .await
    .expect("acercar");
    assert!(
        !api::webhooks::procesar_una_entrega(&state)
            .await
            .expect("sin trabajo"),
        "una entrega failed no se reintenta"
    );

    let (estado, codigo, error): (String, Option<i32>, Option<String>) = sqlx::query_as(
        "SELECT status, response_status, error FROM webhook_deliveries WHERE id = $1",
    )
    .bind(delivery_id)
    .fetch_one(&h.pool)
    .await
    .expect("entrega");
    assert_eq!(estado, "failed");
    assert_eq!(codigo, Some(500), "último response_status registrado");
    assert!(error.is_none(), "un 500 no es error de transporte");

    // 5 intentos = 5 peticiones al receptor.
    assert_eq!(registro.lock().unwrap().len(), 5);
}

#[tokio::test]
async fn el_fallo_de_ejecucion_tambien_se_entrega_con_el_error_estable() {
    let Some(h) = begin().await else { return };
    let (url, registro, _fallos) = receptor().await;

    h.bridge_con_webhook("hook-falla", &url, true).await;
    let execution_id = h.invocar("hook-falla", json!({ "total": 1 })).await;

    let state = AppState {
        pool: h.pool.clone(),
    };
    assert!(
        api::webhooks::procesar_una_entrega(&state)
            .await
            .expect("entrega")
    );

    let recibidas = registro.lock().unwrap().clone();
    assert_eq!(recibidas.len(), 1);
    let payload: Value = serde_json::from_str(&recibidas[0].body).expect("payload JSON");
    assert_eq!(
        payload["success"],
        json!(false),
        "el fallo también se entrega"
    );
    assert_eq!(payload["error"]["code"], json!("BLOCK_FAILED"));
    assert!(
        payload["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("boom-hook")
    );
    assert_eq!(payload["executionId"], json!(execution_id));
    assert_eq!(payload["status"], json!("failed"));
}
