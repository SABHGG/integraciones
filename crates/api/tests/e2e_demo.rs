//! Prueba end-to-end del change `add-mvp1-core` (task 5.2) contra un
//! Postgres **real** y un servidor HTTP local de mentira.
//!
//! Recorre TODO el ciclo de vida de un puente de tres bloques — exactamente
//! el del puente demo (`scripts/seed-demo.sh`) — usando solo la API de
//! administración y el plano de invocación:
//!
//! 1. Bóveda: `POST /api/v1/credentials`.
//! 2. Biblioteca: crear los tres bloques (`http-get`, `http-post`,
//!    `custom_logic`) y publicar sus versiones.
//! 3. Puente: crear borrador, editar trigger + cadena, validar y publicar v1.
//! 4. Invocar `POST /x/{slug}` y verificar el sobre de éxito.
//! 5. Verificar la ejecución y su trace: IO completo por bloque, duración,
//!    credencial resuelta en el host (jamás persistida) y encadenamiento real
//!    de datos (`$.traeUsuario` → bloque custom → `$.enriquece`).
//!
//! Si `DATABASE_URL` no está definida (CI sin base de datos) los tests se
//! omiten con un mensaje y terminan con éxito. En local:
//!
//! ```text
//! ./scripts/dev-postgres.sh
//! DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones cargo test -p api --test e2e_demo
//! ```

use std::sync::{Arc, Mutex};

use api::auth::hash_key;
use api::state::AppState;
use axum::Json;
use axum::Router;
use axum::body::Body;
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
const WS_PREFIX: &str = "e2e-test-";

/// El secreto que vive en la bóveda durante el e2e.
const SECRETO: &str = "sk_e2e_secreto_4d21";

/// TypeScript del bloque custom_logic del puente demo: enriquece al usuario
/// que trae el bloque anterior (nombre en mayúsculas + marca de tiempo).
const CODIGO_ENRIQUECE: &str = r#"
export default (input) => {
  const usuario = input.usuario;
  return {
    cliente: usuario.nombre.toUpperCase(),
    procesadoEn: new Date().toISOString(),
  };
};
"#;

// ── servidor HTTP de mentira con respuestas realistas ─────────────────

/// Una petición recibida por el servidor de mentira.
#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Value,
}

type Registro = Arc<Mutex<Vec<Recorded>>>;

/// GET /users/{id} devuelve un usuario fijo; POST /orders devuelve un pedido.
async fn usuarios(State(registro): State<Registro>, req: Request<Body>) -> Json<Value> {
    let id = req
        .uri()
        .path()
        .rsplit('/')
        .next()
        .unwrap_or("0")
        .to_string();
    let bytes = tomar_y_registrar(&registro, req).await;
    let _ = bytes;
    Json(json!({
        "id": id.parse::<i64>().unwrap_or(0),
        "nombre": "ana",
        "email": "ana@ejemplo.com"
    }))
}

async fn ordenes(State(registro): State<Registro>, req: Request<Body>) -> Json<Value> {
    let bytes = tomar_y_registrar(&registro, req).await;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(
        body["cliente"], "ANA",
        "el puente manda el nombre enriquecido"
    );
    Json(json!({ "orderId": 42, "estado": "creada" }))
}

/// Consume la petición, registra su envolvente cruda y devuelve el cuerpo.
async fn tomar_y_registrar(registro: &Registro, req: Request<Body>) -> axum::body::Bytes {
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
    let body: Value = if bytes.is_empty() {
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
    bytes
}

/// Sirve HTTP en 127.0.0.1:0 y devuelve (dirección, registro de peticiones).
async fn mock_server() -> (String, Registro) {
    let registro: Registro = Arc::new(Mutex::new(Vec::new()));

    let app = Router::new()
        .route("/users/{id}", get(usuarios))
        .route("/orders", post(ordenes))
        .with_state(registro.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("puerto efímero");
    let addr = listener.local_addr().expect("dirección local");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("servidor de mentira");
    });

    (format!("http://{addr}"), registro)
}

// ── arnés ─────────────────────────────────────────────────────────────

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
    /// Pool retenido para futuras aserciones SQL; hoy no se consulta.
    _pool: PgPool,
    _workspace_id: Uuid,
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
    // workspaces se borran en cascada con bloques, puentes, credenciales y keys.
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'e2e-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces previos");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'e2e-test%'")
        .execute(&pool)
        .await
        .expect("limpieza de keys previas");

    let workspace_id = Uuid::now_v7();
    let (key, _) = seed_workspace(&pool, workspace_id).await;

    let app = api::app(AppState { pool: pool.clone() });

    Some(Harness {
        app,
        _pool: pool,
        _workspace_id: workspace_id,
        key,
        _turno: turno,
    })
}

/// Inserta un workspace y una API key vinculada; devuelve la key cruda.
async fn seed_workspace(pool: &PgPool, workspace_id: Uuid) -> (String, Uuid) {
    let slug = format!("{WS_PREFIX}{workspace_id}");
    let key_id = Uuid::now_v7();
    let raw_key = format!("e2e-test-key-{workspace_id}");

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
        .bind(format!("e2e-test-{raw_key}"))
        .bind(hash_key(&raw_key))
        .execute(pool)
        .await
        .expect("api key");

    (raw_key, key_id)
}

impl Harness {
    /// Petición autenticada al plano de administración.
    async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> Resp {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.key))
            .header(header::CONTENT_TYPE, "application/json");
        let body = match body {
            Some(value) => Body::from(value.to_string()),
            None => Body::empty(),
        };
        self.enviar(builder, body).await
    }

    fn post<'a>(&'a self, uri: &'a str, body: Value) -> impl Future<Output = Resp> + 'a {
        self.admin(Method::POST, uri, Some(body))
    }

    fn get<'a>(&'a self, uri: &'a str) -> impl Future<Output = Resp> + 'a {
        self.admin(Method::GET, uri, None)
    }

    /// Petición al plano de invocación `/x/{slug}` (sin auth de administración).
    async fn x(&self, slug: &str, body: Value) -> Resp {
        let builder = Request::builder()
            .method(Method::POST)
            .uri(format!("/x/{slug}"))
            .header(header::CONTENT_TYPE, "application/json");
        self.enviar(builder, Body::from(body.to_string())).await
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
}

use std::future::Future;

/// El viaje completo: bóveda → biblioteca → puente → invocación → trace.
#[tokio::test]
async fn el_puente_demo_recorre_biblioteca_publicacion_invocacion_y_trace() {
    let Some(h) = begin().await else { return };
    let (base, registro) = mock_server().await;

    // ── 1. Bóveda ──────────────────────────────────────────────────────
    let credencial = h
        .post(
            "/api/v1/credentials",
            json!({ "name": "pago-api", "secret": SECRETO, "description": "key del proveedor" }),
        )
        .await;
    assert_eq!(
        credencial.status,
        StatusCode::CREATED,
        "{}",
        credencial.body
    );

    // ── 2. Biblioteca: tres bloques, tres versiones publicadas ────────
    let bloque_get = h
        .post(
            "/api/v1/blocks",
            json!({
                "slug": "http-get",
                "name": "HTTP GET",
                "blockType": "http",
                "category": "http",
                "description": "Llama a una URL por GET",
                "icon": "globe"
            }),
        )
        .await;
    assert_eq!(
        bloque_get.status,
        StatusCode::CREATED,
        "{}",
        bloque_get.body
    );
    let get_id = bloque_get.json()["id"].as_str().unwrap().to_string();

    let bloque_post = h
        .post(
            "/api/v1/blocks",
            json!({
                "slug": "http-post",
                "name": "HTTP POST",
                "blockType": "http",
                "category": "http",
                "description": "Llama a una URL por POST con body JSON",
                "icon": "send"
            }),
        )
        .await;
    assert_eq!(
        bloque_post.status,
        StatusCode::CREATED,
        "{}",
        bloque_post.body
    );
    let post_id = bloque_post.json()["id"].as_str().unwrap().to_string();

    let bloque_custom = h
        .post(
            "/api/v1/blocks",
            json!({
                "slug": "enriquece-cliente",
                "name": "Enriquece cliente",
                "blockType": "custom_logic",
                "category": "transform",
                "description": "Pone el nombre del usuario en mayúsculas",
                "icon": "wand"
            }),
        )
        .await;
    assert_eq!(
        bloque_custom.status,
        StatusCode::CREATED,
        "{}",
        bloque_custom.body
    );
    let custom_id = bloque_custom.json()["id"].as_str().unwrap().to_string();

    // Los built-in publican sin código (solo schemas); el custom_logic, con él.
    let version_get = h
        .post(
            &format!("/api/v1/blocks/{get_id}/versions"),
            json!({ "outputSchema": { "type": "object" } }),
        )
        .await;
    assert_eq!(
        version_get.status,
        StatusCode::CREATED,
        "{}",
        version_get.body
    );
    assert!(version_get.json()["code"].is_null());
    let version_get_id = version_get.json()["id"].as_str().unwrap().to_string();

    let version_post = h
        .post(
            &format!("/api/v1/blocks/{post_id}/versions"),
            json!({ "outputSchema": { "type": "object" } }),
        )
        .await;
    assert_eq!(
        version_post.status,
        StatusCode::CREATED,
        "{}",
        version_post.body
    );
    let version_post_id = version_post.json()["id"].as_str().unwrap().to_string();

    let version_custom = h
        .post(
            &format!("/api/v1/blocks/{custom_id}/versions"),
            json!({
                "code": CODIGO_ENRIQUECE,
                "inputSchema": {
                    "type": "object",
                    "properties": { "usuario": { "type": "object" } },
                    "required": ["usuario"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "cliente": { "type": "string" },
                        "procesadoEn": { "type": "string" }
                    },
                    "required": ["cliente", "procesadoEn"]
                }
            }),
        )
        .await;
    assert_eq!(
        version_custom.status,
        StatusCode::CREATED,
        "{}",
        version_custom.body
    );
    assert!(
        version_custom.json()["code"]
            .as_str()
            .unwrap()
            .contains("export default")
    );
    let version_custom_id = version_custom.json()["id"].as_str().unwrap().to_string();

    // ── 3. Puente: borrador → trigger + cadena → validar → publicar ───
    let puente = h
        .post(
            "/api/v1/bridges",
            json!({ "slug": "registro-cliente", "name": "Registro de cliente" }),
        )
        .await;
    assert_eq!(puente.status, StatusCode::CREATED, "{}", puente.body);
    let puente_id = puente.json()["id"].as_str().unwrap().to_string();

    let borrador = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/registro-cliente" },
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": { "type": "integer" },
                "items": { "type": "array", "items": { "type": "integer" } }
            },
            "required": ["id", "items"]
        },
        "blocks": [
            {
                "id": "b1",
                "name": "traeUsuario",
                "blockId": get_id,
                "blockVersionId": version_get_id,
                "config": {
                    "url": format!("{base}/users/{{$.trigger.body.id}}"),
                    "method": "GET",
                    "headers": { "X-Api-Key": "{$credential:pago-api}" }
                }
            },
            {
                "id": "b2",
                "name": "enriquece",
                "blockId": custom_id,
                "blockVersionId": version_custom_id,
                "input": { "usuario": "$.traeUsuario" }
            },
            {
                "id": "b3",
                "name": "creaOrden",
                "blockId": post_id,
                "blockVersionId": version_post_id,
                "config": {
                    "url": format!("{base}/orders"),
                    "method": "POST",
                    "body": {
                        "cliente": "$.enriquece.cliente",
                        "items": "$.trigger.body.items"
                    }
                }
            }
        ]
    });
    let editado = h
        .admin(
            Method::PATCH,
            &format!("/api/v1/bridges/{puente_id}"),
            Some(borrador.clone()),
        )
        .await;
    assert_eq!(editado.status, StatusCode::OK, "{}", editado.body);

    let validado = h
        .admin(
            Method::POST,
            &format!("/api/v1/bridges/{puente_id}/validate"),
            None,
        )
        .await;
    assert_eq!(validado.status, StatusCode::OK, "{}", validado.body);
    assert_eq!(validado.json()["valid"], true, "{}", validado.body);
    assert_eq!(validado.json()["errors"], json!([]));

    let publicado = h
        .admin(
            Method::POST,
            &format!("/api/v1/bridges/{puente_id}/versions"),
            None,
        )
        .await;
    assert_eq!(publicado.status, StatusCode::CREATED, "{}", publicado.body);
    let snapshot = publicado.json();
    assert_eq!(snapshot["version"], 1);
    let bloques = snapshot["definition"]["blocks"].as_array().expect("cadena");
    assert_eq!(bloques.len(), 3);
    assert!(
        bloques[0]["code"].is_null(),
        "el built-in http no lleva code"
    );
    assert!(
        bloques[1]["code"]
            .as_str()
            .unwrap()
            .contains("export default"),
        "el custom_logic lleva su JS en el snapshot"
    );

    // ── 4. Invocación: el disparo encadena los tres bloques ───────────
    let respuesta = h
        .x("registro-cliente", json!({ "id": 7, "items": [1, 2, 3] }))
        .await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], true, "{}", sobre);
    assert_eq!(sobre["data"], json!({ "orderId": 42, "estado": "creada" }));
    let execution_id = sobre["executionId"]
        .as_str()
        .expect("executionId")
        .to_string();

    // ── 5. La ejecución y su trace cuentan todo el viaje ──────────────
    let detalle = h
        .get(&format!("/api/v1/executions/{execution_id}"))
        .await
        .json();
    assert_eq!(detalle["status"], "succeeded", "{}", detalle);
    assert_eq!(detalle["triggerType"], "http");
    assert_eq!(
        detalle["triggerData"]["body"],
        json!({ "id": 7, "items": [1, 2, 3] })
    );
    assert_eq!(detalle["bridgeId"], puente_id);
    assert!(detalle["bridgeVersionId"].as_str().is_some());

    // El host hizo exactamente dos llamadas: GET con el secreto, POST con el
    // cliente enriquecido y los ítems enteros del trigger.
    let recibidas = registro.lock().unwrap().clone();
    assert_eq!(recibidas.len(), 2, "{recibidas:?}");
    assert_eq!(recibidas[0].method, "GET");
    assert_eq!(recibidas[0].path, "/users/7");
    let api_key = recibidas[0]
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-api-key"))
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    assert_eq!(api_key, SECRETO, "el GET lleva la credencial de la bóveda");
    assert_eq!(recibidas[1].method, "POST");
    assert_eq!(recibidas[1].path, "/orders");
    assert_eq!(
        recibidas[1].body,
        json!({ "cliente": "ANA", "items": [1, 2, 3] }),
        "los refs encadenan tipos (array) y el nombre sale del custom_logic"
    );

    // Trace: tres pasos succeeded con IO completo y duración.
    let trace = h
        .get(&format!("/api/v1/executions/{execution_id}/trace"))
        .await
        .json();
    let steps = trace["steps"].as_array().expect("pasos");
    assert_eq!(steps.len(), 3, "{}", trace);

    assert_eq!(steps[0]["blockName"], "traeUsuario");
    assert_eq!(steps[0]["status"], "succeeded");
    assert_eq!(steps[0]["output"]["nombre"], "ana", "salida real del GET");
    assert!(steps[0]["durationMs"].is_u64(), "{}", steps[0]);

    assert_eq!(steps[1]["blockName"], "enriquece");
    assert_eq!(steps[1]["status"], "succeeded");
    assert_eq!(
        steps[1]["input"]["usuario"]["nombre"], "ana",
        "entrada resuelta del paso previo"
    );
    assert_eq!(
        steps[1]["output"]["cliente"], "ANA",
        "el custom_logic transformó"
    );
    assert!(steps[1]["output"]["procesadoEn"].is_string());

    assert_eq!(steps[2]["blockName"], "creaOrden");
    assert_eq!(steps[2]["status"], "succeeded");
    assert_eq!(
        steps[2]["input"],
        json!({}),
        "el built-in no pide input de instancia"
    );
    assert_eq!(
        steps[2]["output"],
        json!({ "orderId": 42, "estado": "creada" })
    );

    // El secreto jamás queda en el IO persistido.
    let trace_text = trace.to_string();
    assert!(
        !trace_text.contains(SECRETO),
        "el secreto no puede aparecer en el trace: {trace_text}"
    );
}
