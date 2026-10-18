//! Tests de integración de la bóveda de credenciales (task 4.7) contra un
//! Postgres **real**: el secreto es write-only y se guarda cifrado en reposo.
//!
//! Montan la misma aplicación axum que producción (`api::app`) y la ejercen a
//! través del router completo (auth Bearer incluida) con `tower::ServiceExt`.
//!
//! Si `DATABASE_URL` no está definida (CI sin base de datos) los tests se
//! omiten con un mensaje y terminan con éxito. En local:
//!
//! ```text
//! ./scripts/dev-postgres.sh
//! DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones cargo test -p api --test credentials_api
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
const WS_PREFIX: &str = "cred-test-";

/// El secreto que usan todos los tests.
const SECRETO: &str = "sk_test_secreto_9f3b";

/// La rotación del secreto.
const SECRETO_NUEVO: &str = "sk_test_rotado_77ab";

/// Una petición recibida por el servidor de mentira.
#[derive(Clone, Debug)]
struct Recorded {
    headers: Vec<(String, String)>,
}

type Registro = Arc<Mutex<Vec<Recorded>>>;

/// Servidor de mentira: registra la petición y devuelve un eco fijo.
async fn captura(State(registro): State<Registro>, req: Request<Body>) -> Json<Value> {
    let headers_vec: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect();
    registro.lock().unwrap().push(Recorded {
        headers: headers_vec,
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
        axum::serve(listener, app)
            .await
            .expect("servidor de mentira");
    });

    (format!("http://{addr}"), registro)
}

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

    // Limpia solo las filas creadas por estos tests (prefijo dedicado).
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'cred-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces previos");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'cred-test%'")
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
    let raw_key = format!("cred-test-key-{workspace_id}");

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
        .bind(format!("cred-test-{raw_key}"))
        .bind(hash_key(&raw_key))
        .execute(pool)
        .await
        .expect("api key");

    (raw_key, key_id)
}

impl Harness {
    /// Petición al plano de administración (con auth Bearer).
    async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> Resp {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.key));
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

    /// Crea una credencial por la API y devuelve el id.
    async fn post_credencial(&self, name: &str, secret: &str) -> Resp {
        self.admin(
            Method::POST,
            "/api/v1/credentials",
            Some(json!({
                "name": name,
                "secret": secret,
            })),
        )
        .await
    }

    /// Lee los bytes crudos de la bóveda.
    async fn bytes_boveda(&self, name: &str) -> Vec<u8> {
        sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT secret_encrypted FROM credentials WHERE workspace_id = $1 AND name = $2",
        )
        .bind(self.workspace_id)
        .bind(name)
        .fetch_one(&self.pool)
        .await
        .expect("credencial sembrada")
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
            content_type: application_json(),
            body: String::from_utf8(bytes.to_vec()).expect("cuerpo UTF-8"),
        }
    }

    /// Siembra un puente publicado (v1) con las instancias dadas (bloques
    /// `http` con `code: null`: su comportamiento es del host).
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
}

fn application_json() -> String {
    "application/json".to_string()
}

// ── write-only: el secreto jamás sale por la API ─────────────────────────

#[tokio::test]
async fn crea_credencial_y_nunca_devuelve_el_secreto() {
    let Some(h) = begin().await else { return };

    let creacion = h.post_credencial("stripe", SECRETO).await;
    assert_eq!(creacion.status, StatusCode::CREATED, "{}", creacion.body);
    let credencial = creacion.json();
    assert!(
        !credencial["id"].as_str().unwrap_or_default().is_empty(),
        "id: {}",
        creacion.body
    );
    assert_eq!(credencial["name"], "stripe");
    assert!(
        !creacion.body.contains(SECRETO),
        "el secreto jamás se devuelve en la creacion: {}",
        creacion.body
    );

    let listado = h.admin(Method::GET, "/api/v1/credentials", None).await;
    assert_eq!(listado.status, StatusCode::OK, "{}", listado.body);
    assert!(
        !listado.body.contains(SECRETO),
        "el listado no devuelve el valor: {}",
        listado.body
    );

    let id = credencial["id"].as_str().expect("id");
    let detalle = h
        .admin(Method::GET, &format!("/api/v1/credentials/{id}"), None)
        .await;
    assert_eq!(detalle.status, StatusCode::OK, "{}", detalle.body);
    assert_eq!(detalle.json()["name"], "stripe");
    assert!(
        !detalle.body.contains(SECRETO),
        "el detalle no devuelve el valor: {}",
        detalle.body
    );
}

#[tokio::test]
async fn el_secreto_se_guarda_cifrado_en_reposo() {
    let Some(h) = begin().await else { return };

    h.post_credencial("stripe", SECRETO).await;

    let bytes = h.bytes_boveda("stripe").await;
    assert!(!bytes.is_empty(), "hay bytes en la bóveda");
    assert!(
        !bytes
            .windows(SECRETO.len())
            .any(|window| window == SECRETO.as_bytes()),
        "el secreto en claro no puede vivir en la tabla: {}",
        String::from_utf8_lossy(&bytes)
    );
}

#[tokio::test]
async fn rotacion_basica_los_puentes_usan_el_nuevo_valor() {
    let Some(h) = begin().await else { return };
    let (base, registro) = mock_server().await;
    h.post_credencial("stripe", SECRETO).await;

    h.bridge(
        "trae-usuario",
        json!([{
            "id": "b1",
            "name": "traeUsuario",
            "code": null,
            "config": {
                "url": format!("{base}/users/7"),
                "method": "GET",
                "headers": { "X-Api-Key": "{$credential:stripe}" }
            }
        }]),
    )
    .await;

    // Primera invocación: el puente ve el secreto original.
    let sobre_v1 = h.x(Method::POST, "trae-usuario", Some(json!({}))).await;
    assert_eq!(sobre_v1.status, StatusCode::OK, "{}", sobre_v1.body);
    assert_eq!(sobre_v1.json()["success"], true, "{}", sobre_v1.body);
    let recibidas_v1 = registro.lock().unwrap().clone();
    let header_v1 = recibidas_v1[0]
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-api-key"))
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    assert_eq!(header_v1, SECRETO, "el puente resuelve el secreto v1");
    registro.lock().unwrap().clear();

    // Rotación por PATCH: sin cambio en los puentes.
    let listado = h.admin(Method::GET, "/api/v1/credentials", None).await;
    let listado = listado.json();
    let id = listado["credentials"][0]["id"]
        .as_str()
        .expect("id del listado")
        .to_string();
    let rotacion = h
        .admin(
            Method::PATCH,
            &format!("/api/v1/credentials/{id}"),
            Some(json!({
                "secret": SECRETO_NUEVO,
            })),
        )
        .await;
    assert_eq!(rotacion.status, StatusCode::OK, "{}", rotacion.body);
    assert!(
        !rotacion.body.contains(SECRETO_NUEVO),
        "la rotación tampoco devuelve el valor: {}",
        rotacion.body
    );
    registro.lock().unwrap().clear();

    // Segunda invocación: el mismo puente usa el secreto rotado.
    let sobre_v2 = h.x(Method::POST, "trae-usuario", Some(json!({}))).await;
    assert_eq!(sobre_v2.status, StatusCode::OK, "{}", sobre_v2.body);
    let recibidas_v2 = registro.lock().unwrap().clone();
    let header_v2 = recibidas_v2[0]
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-api-key"))
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    assert_eq!(
        header_v2, SECRETO_NUEVO,
        "los puentes usan el secreto rotado"
    );
}

// ── errores del plano admin: siempre Problem Details ─────────────────────

#[tokio::test]
async fn post_con_nombre_duplicado_responde_409_problem() {
    let Some(h) = begin().await else { return };

    h.post_credencial("stripe", SECRETO).await;
    let duplicado = h.post_credencial("stripe", SECRETO).await;
    let problema = duplicado.problem(StatusCode::CONFLICT);
    assert!(
        problema["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("stripe"),
        "el detalle menciona el nombre: {}",
        duplicado.body
    );
}

#[tokio::test]
async fn post_con_nombre_vacio_responde_400_problem() {
    let Some(h) = begin().await else { return };

    let vacio = h.post_credencial("", SECRETO).await;
    vacio.problem(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn delete_elimina_la_credencial_y_luega_404() {
    let Some(h) = begin().await else { return };

    h.post_credencial("stripe", SECRETO).await;
    let listado = h.admin(Method::GET, "/api/v1/credentials", None).await;
    let listado = listado.json();
    let id = listado["credentials"][0]["id"]
        .as_str()
        .expect("id del listado")
        .to_string();

    let borrado = h
        .admin(Method::DELETE, &format!("/api/v1/credentials/{id}"), None)
        .await;
    assert_eq!(borrado.status, StatusCode::NO_CONTENT, "{}", borrado.body);

    let detalle = h
        .admin(Method::GET, &format!("/api/v1/credentials/{id}"), None)
        .await;
    detalle.problem(StatusCode::NOT_FOUND);
}
