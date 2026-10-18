//! Tests de integración de los bloques built-in `http` (task 4.6) contra un
//! Postgres **real** y un servidor HTTP local de mentira.
//!
//! Cubren: ejecución en el host de instancias sin `code` (bloques
//! `http-get`/`http-post` de la biblioteca base) con config interpolada
//! (`{$.ref}`), resolución de credenciales `{$credential:nombre}` desde la
//! bóveda del workspace (el secreto jamás entra al contexto ni al IO
//! persistido) y `ctx.credential(name)` desde bloques CUSTOM_LOGIC.
//!
//! Si `DATABASE_URL` no está definida (CI sin base de datos) los tests se
//! omiten con un mensaje y terminan con éxito. En local:
//!
//! ```text
//! ./scripts/dev-postgres.sh
//! DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones cargo test -p api
//! ```

use std::sync::{Arc, Mutex};

use api::auth::hash_key;
use api::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::Json;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode, header};
use axum::routing::{get, post};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

/// Los tests comparten base de datos: se serializan para que la limpieza de
/// fixtures de un test no borre las filas de otro.
static DB_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// Prefijo exclusivo de estos tests (limpieza por cascada de workspaces).
const WS_PREFIX: &str = "builtin-test-";

/// Una petición recibida por el servidor de mentira.
#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Value,
}

type Registro = Arc<Mutex<Vec<Recorded>>>;

/// Handler del servidor de mentira: registra la llamada (petición cruda) y
/// devuelve un eco fijo.
async fn captura(State(registro): State<Registro>, req: Request<Body>) -> Json<Value> {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let mut headers_vec: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect();
    headers_vec.sort();
    let bytes = axum::body::to_bytes(req.into_body(), 1 << 20)
        .await
        .unwrap_or_default();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::String("no-json".into()))
    };
    registro.lock().unwrap().push(Recorded {
        method,
        path,
        headers: headers_vec,
        body,
    });
    Json(json!({ "eco": true, "ruta": "ok" }))
}

/// Sirve HTTP en 127.0.0.1:0 y devuelve (dirección, registro de peticiones).
async fn mock_server() -> (String, Registro) {
    let registro: Registro = Arc::new(Mutex::new(Vec::new()));

    let app = Router::new()
        .route("/users/{id}", get(captura))
        .route("/orders", post(captura))
        .with_state(registro.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("puerto efímero");
    let addr = listener.local_addr().expect("dirección local");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("servidor de mentira");
    });

    (format!("http://{addr}"), registro)
}

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

    // Limpia solo las filas creadas por estos tests (prefijo dedicado): los
    // workspaces se borran en cascada con sus bloques, puentes, credenciales y keys.
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'builtin-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces previos");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'builtin-test%'")
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
    let raw_key = format!("builtin-test-key-{workspace_id}");

    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace de test")
        .bind(&slug)
        .execute(pool)
        .await
        .expect("workspace");

    sqlx::query(
        "INSERT INTO api_keys (id, workspace_id, name, key_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(key_id)
    .bind(workspace_id)
    .bind(format!("builtin-test-{raw_key}"))
    .bind(hash_key(&raw_key))
    .execute(pool)
    .await
    .expect("api key");

    (raw_key, key_id)
}

/// El secreto que usan todos los tests de credenciales.
const SECRETO: &str = "sk_test_secreto_9f3b";

impl Harness {
    /// Siembra la credencial `stripe` de la bóveda del workspace.
    async fn credencial(&self, name: &str) {
        sqlx::query(
            "INSERT INTO credentials (id, workspace_id, name, secret_encrypted) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(Uuid::now_v7())
        .bind(self.workspace_id)
        .bind(name)
        .bind(SECRETO.as_bytes())
        .execute(&self.pool)
        .await
        .expect("credencial sembrada");
    }

    /// Petición al plano de invocación `/x/{slug}` (sin auth de administración).
    async fn x(&self, method: Method, slug: &str, body: Option<Value>) -> Resp {
        let builder = Request::builder()
            .method(method)
            .uri(format!("/x/{slug}"))
            .header(header::CONTENT_TYPE, "application/json");
        let body = match body {
            Some(value) => Body::from(value.to_string()),
            None => Body::empty(),
        };
        self.enviar(builder, body).await
    }

    /// Petición al plano de administración (con auth Bearer).
    async fn admin_get(&self, uri: &str) -> Resp {
        let builder = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.key));
        self.enviar(builder, Body::empty()).await
    }

    async fn enviar(&self, builder: axum::http::request::Builder, body: Body) -> Resp {
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).expect("petición válida"))
            .await
            .expect("respuesta del router");

        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
            .await
            .expect("cuerpo legible");
        Resp {
            status,
            body: String::from_utf8(bytes.to_vec()).expect("cuerpo UTF-8"),
        }
    }

    /// Siembra un puente publicado (v1) con las instancias dadas (los bloques
    /// `http` llevan `code: null`: su comportamiento es del host).
    async fn bridge(&self, slug: &str, blocks: Value) {
        let bridge_id = Uuid::now_v7();
        let definition = json!({
            "trigger": { "type": "http", "method": "POST", "path": format!("/{slug}") },
            "blocks": blocks,
        });

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

    /// Trace de una ejecución.
    async fn trace(&self, execution_id: &str) -> Value {
        self.admin_get(&format!("/api/v1/executions/{execution_id}/trace"))
            .await
            .json()
    }
}

#[tokio::test]
async fn bloque_http_get_resuelve_config_y_devuelve_el_cuerpo() {
    let Some(h) = begin().await else { return };
    let (base, registro) = mock_server().await;
    h.credencial("stripe").await;

    h.bridge(
        "trae-usuario",
        json!([{
            "id": "b1",
            "name": "traeUsuario",
            "code": null,
            "input": { "id": "$.trigger.body.id" },
            "config": {
                "url": format!("{base}/users/{{$.trigger.body.id}}"),
                "method": "GET",
                "headers": { "X-Api-Key": "{$credential:stripe}" }
            }
        }]),
    )
    .await;

    let respuesta = h.x(Method::POST, "trae-usuario", Some(json!({ "id": 7 }))).await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], true, "{}", sobre);
    assert_eq!(sobre["data"], json!({ "eco": true, "ruta": "ok" }));

    // El host hizo la petición con el header resuelto desde la bóveda.
    let recibidas = registro.lock().unwrap().clone();
    assert_eq!(recibidas.len(), 1, "{recibidas:?}");
    let llamada = &recibidas[0];
    assert_eq!(llamada.method, "GET");
    assert_eq!(llamada.path, "/users/7");
    let api_key = llamada
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-api-key"))
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    assert_eq!(api_key, SECRETO, "el header lleva el secreto resuelto");

    // El secreto NO entra al IO persistido: input ni output del paso.
    let execution_id = sobre["executionId"].as_str().expect("executionId");
    let trace = h.trace(execution_id).await;
    let trace_text = trace.to_string();
    assert!(
        !trace_text.contains(SECRETO),
        "el secreto no puede aparecer en el trace: {trace_text}"
    );
    let steps = trace["steps"].as_array().expect("pasos");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["status"], "succeeded");
    assert_eq!(steps[0]["input"], json!({ "id": 7 }));
    assert_eq!(steps[0]["output"], json!({ "eco": true, "ruta": "ok" }));
}

#[tokio::test]
async fn bloque_http_post_envia_body_json_interpolado() {
    let Some(h) = begin().await else { return };
    let (base, registro) = mock_server().await;
    h.credencial("stripe").await;

    h.bridge(
        "crea-orden",
        json!([{
            "id": "b1",
            "name": "creaOrden",
            "code": null,
            "input": { "total": "$.trigger.body.total" },
            "config": {
                "url": format!("{base}/orders"),
                "method": "POST",
                "headers": { "Authorization": "Bearer {$credential:stripe}" },
                "body": { "total": "$.trigger.body.total", "items": [1, 2, 3] }
            }
        }]),
    )
    .await;

    let respuesta = h
        .x(Method::POST, "crea-orden", Some(json!({ "total": 42 })))
        .await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], true, "{}", sobre);

    let recibidas = registro.lock().unwrap().clone();
    assert_eq!(recibidas.len(), 1, "{recibidas:?}");
    let llamada = &recibidas[0];
    assert_eq!(llamada.method, "POST");
    assert_eq!(llamada.path, "/orders");
    let auth = llamada
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    assert_eq!(auth, format!("Bearer {SECRETO}"));
    assert_eq!(
        llamada.body,
        json!({ "total": 42, "items": [1, 2, 3] }),
        "el body de config viaja interpolado y tipado"
    );

    // El secreto tampoco vive en el detalle de la ejecución.
    let execution_id = sobre["executionId"].as_str().expect("executionId");
    let detalle = h.admin_get(&format!("/api/v1/executions/{execution_id}")).await;
    assert!(
        !detalle.body.contains(SECRETO),
        "el secreto no puede aparecer en el detalle: {}",
        detalle.body
    );
}

#[tokio::test]
async fn credencial_inexistente_falla_el_bloque_sin_salir_a_la_red() {
    let Some(h) = begin().await else { return };
    let (base, registro) = mock_server().await;

    h.bridge(
        "con-credencial-fantasma",
        json!([{
            "id": "b1",
            "name": "llama",
            "code": null,
            "input": {},
            "config": {
                "url": format!("{base}/users/1"),
                "method": "GET",
                "headers": { "X-Api-Key": "{$credential:no_existe}" }
            }
        }]),
    )
    .await;

    let respuesta = h
        .x(Method::POST, "con-credencial-fantasma", Some(json!({})))
        .await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], false, "{}", sobre);
    assert_eq!(sobre["error"]["code"], "BLOCK_FAILED");
    let mensaje = sobre["error"]["message"].as_str().expect("mensaje");
    assert!(
        mensaje.contains("no_existe"),
        "el mensaje menciona la credencial ausente: {mensaje}"
    );

    // La petición nunca salió al host remoto.
    let recibidas = registro.lock().unwrap().clone();
    assert!(recibidas.is_empty(), "no debió salir a la red: {recibidas:?}");

    // La ejecución quedó failed y el paso registra el fallo.
    let execution_id = sobre["executionId"].as_str().expect("executionId");
    let trace = h.trace(execution_id).await;
    let steps = trace["steps"].as_array().expect("pasos");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["status"], "failed");
}

#[tokio::test]
async fn ctx_credential_resuelve_desde_la_boveda_en_custom_logic() {
    let Some(h) = begin().await else { return };
    h.credencial("stripe").await;

    let js = "export default function(input, ctx) {\n    const token = ctx.credential(\"stripe\");\n    return { largo: token.length, prefijo: token.slice(0, 8) };\n}";
    h.bridge(
        "usa-credencial",
        json!([{
            "id": "b1",
            "name": "leeSecreto",
            "code": js,
            "input": {},
            "config": {}
        }]),
    )
    .await;

    let respuesta = h
        .x(Method::POST, "usa-credencial", Some(json!({})))
        .await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], true, "{}", sobre);
    assert_eq!(
        sobre["data"],
        json!({ "largo": SECRETO.len(), "prefijo": &SECRETO[..8] }),
        "ctx.credential resuelve el secreto de la bóveda del workspace"
    );
}
