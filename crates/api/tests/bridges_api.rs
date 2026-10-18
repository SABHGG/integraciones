//! Tests de integración de la API de puentes contra un Postgres **real**.
//!
//! Montan la misma aplicación axum que producción (`api::app`) y la ejercen a
//! través del router completo (auth Bearer incluida) con `tower::ServiceExt`:
//! CRUD del borrador, `validate` acumulativo y publicación de versiones con
//! snapshot resuelto.
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
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'bridge-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces previos");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'bridge-test%'")
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
    let slug = format!("bridge-test-{workspace_id}");
    let key_id = Uuid::now_v7();
    let raw_key = format!("bridge-test-key-{workspace_id}");

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
        .bind(format!("bridge-test-{raw_key}"))
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

    async fn post(&self, uri: &str, body: Option<Value>) -> Resp {
        self.send(Method::POST, uri, Some(&self.key), body).await
    }

    async fn patch(&self, uri: &str, body: Value) -> Resp {
        self.send(Method::PATCH, uri, Some(&self.key), Some(body))
            .await
    }

    async fn delete(&self, uri: &str) -> Resp {
        self.send(Method::DELETE, uri, Some(&self.key), None).await
    }

    /// Crea un puente vía API y devuelve su JSON (asserta 201).
    async fn create_bridge(&self, slug: &str) -> Value {
        let response = self
            .post(
                "/api/v1/bridges",
                Some(json!({ "slug": slug, "name": format!("Puente {slug}") })),
            )
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
        response.json()
    }

    /// Crea un bloque `custom_logic` y publica una versión con schemas dados;
    /// devuelve `(blockId, blockVersionId)`.
    async fn published_block(
        &self,
        slug: &str,
        input_schema: Value,
        output_schema: Value,
    ) -> (Uuid, Uuid) {
        let response = self
            .post(
                "/api/v1/blocks",
                Some(json!({ "slug": slug, "name": slug, "blockType": "custom_logic" })),
            )
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
        let block_id = response.json()["id"]
            .as_str()
            .expect("id del bloque")
            .to_string();

        let published = self
            .post(
                &format!("/api/v1/blocks/{block_id}/versions"),
                Some(json!({
                    "code": "export default (input) => input;",
                    "inputSchema": input_schema,
                    "outputSchema": output_schema,
                })),
            )
            .await;
        assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
        let version_id = published.json()["id"]
            .as_str()
            .expect("id de la versión")
            .to_string();

        (
            Uuid::parse_str(&block_id).expect("uuid del bloque"),
            Uuid::parse_str(&version_id).expect("uuid de la versión"),
        )
    }

    /// Crea un workspace extra con su key; devuelve `(workspace_id, key cruda)`.
    async fn extra_workspace(&self) -> (Uuid, String) {
        let id = Uuid::now_v7();
        let (key, _) = seed_workspace(&self.pool, id).await;
        (id, key)
    }

    /// Instancia de borrador lista para usar.
    fn instance(&self, id: &str, name: &str, block: &(Uuid, Uuid), input: Value) -> Value {
        json!({
            "id": id,
            "name": name,
            "blockId": block.0.to_string(),
            "blockVersionId": block.1.to_string(),
            "input": input,
        })
    }
}

/// Bloque "origen" con `total` (integer) y `cliente.email` (string) en su salida.
async fn bloque_origen(h: &Harness, suffix: &str) -> (Uuid, Uuid) {
    h.published_block(
        &format!("origen-{suffix}"),
        json!({}),
        json!({
            "type": "object",
            "properties": {
                "total": { "type": "integer" },
                "cliente": { "type": "object", "properties": { "email": { "type": "string" } } }
            }
        }),
    )
    .await
}

/// Bloque "destino" que espera `total` (integer) en su entrada.
async fn bloque_destino(h: &Harness, suffix: &str) -> (Uuid, Uuid) {
    h.published_block(
        &format!("destino-{suffix}"),
        json!({ "type": "object", "properties": { "total": { "type": "integer" } } }),
        json!({}),
    )
    .await
}

#[tokio::test]
async fn el_flujo_completo_crea_valida_publica_y_versiona() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "flujo").await;
    let destino = bloque_destino(&h, "flujo").await;

    // Crear: borrador vacío, latestVersion null.
    let bridge = h.create_bridge("flujo-completo").await;
    let id = bridge["id"].as_str().unwrap().to_string();
    assert_eq!(bridge["slug"], "flujo-completo");
    assert!(bridge["latestVersion"].is_null());
    assert!(
        bridge["draft"].is_object(),
        "el borrador nace como objeto vacío"
    );
    let location = h.get(&format!("/api/v1/bridges/{id}")).await;
    assert_eq!(location.status, StatusCode::OK);

    // Editar el borrador: trigger http + cadena de dos bloques.
    let draft = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/flujo" },
        "inputSchema": {},
        "blocks": [
            h.instance("paso-origen", "origen", &origen, json!({})),
            h.instance("paso-destino", "destino", &destino, json!({ "total": "$.origen.total" })),
        ]
    });
    let patched = h
        .patch(&format!("/api/v1/bridges/{id}"), json!({ "draft": draft }))
        .await;
    // `draft` no es un campo del PATCH: las secciones van sueltas.
    assert_eq!(
        patched.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        patched.body
    );

    let patched = h
        .patch(&format!("/api/v1/bridges/{id}"), draft.clone())
        .await;
    assert_eq!(patched.status, StatusCode::OK, "{}", patched.body);
    assert_eq!(patched.json()["draft"]["trigger"]["method"], "POST");

    // Validar: la cadena es correcta.
    let validated = h
        .post(&format!("/api/v1/bridges/{id}/validate"), None)
        .await;
    assert_eq!(validated.status, StatusCode::OK, "{}", validated.body);
    assert_eq!(validated.json()["valid"], true, "{}", validated.body);
    assert_eq!(validated.json()["errors"], json!([]));

    // Publicar la versión 1: snapshot resuelto.
    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    let version = published.json();
    assert_eq!(version["version"], 1);
    assert_eq!(version["definition"]["trigger"]["method"], "POST");
    assert_eq!(version["definition"]["trigger"]["path"], "/x/flujo");
    assert_eq!(version["definition"]["blocks"][0]["name"], "origen");
    // El code de la block_version quedó copiado al snapshot.
    let code = version["definition"]["blocks"][0]["code"]
        .as_str()
        .expect("code resuelto en el snapshot");
    assert!(code.contains("export default"), "code resuelto: {code}");
    // El inputSchema del destino viene del bloque, no del borrador.
    assert_eq!(
        version["definition"]["blocks"][1]["inputSchema"]["properties"]["total"]["type"],
        "integer"
    );

    let bridge = h.get(&format!("/api/v1/bridges/{id}")).await.json();
    assert_eq!(bridge["latestVersion"], 1);

    // Segunda publicación: v2 tras editar el borrador.
    let mut draft2 = draft.clone();
    draft2["blocks"][1]["config"] = json!({ "nota": "cambio trivial" });
    h.patch(&format!("/api/v1/bridges/{id}"), draft2).await;
    let second = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    assert_eq!(second.status, StatusCode::CREATED, "{}", second.body);
    assert_eq!(second.json()["version"], 2);
    assert_eq!(
        h.get(&format!("/api/v1/bridges/{id}")).await.json()["latestVersion"],
        2
    );

    // El listado de versiones va más nueva primero.
    let versions = h.get(&format!("/api/v1/bridges/{id}/versions")).await;
    assert_eq!(versions.status, StatusCode::OK, "{}", versions.body);
    let page = versions.json();
    assert_eq!(page["items"][0]["version"], 2);
    assert_eq!(page["items"][1]["version"], 1);
    assert!(page["nextCursor"].is_null());
}

#[tokio::test]
async fn el_slug_duplicado_en_el_mismo_workspace_es_409() {
    let Some(h) = begin().await else { return };

    h.create_bridge("slug-repetido").await;
    let response = h
        .post(
            "/api/v1/bridges",
            Some(json!({ "slug": "slug-repetido", "name": "otro" })),
        )
        .await;
    let problem = response.problem(StatusCode::CONFLICT);
    assert!(
        problem["detail"]
            .as_str()
            .unwrap()
            .contains("slug-repetido")
    );
}

#[tokio::test]
async fn validate_acumula_todos_los_errores_del_borrador() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "acumula").await;
    let destino = bloque_destino(&h, "acumula").await;

    let bridge = h.create_bridge("acumula-errores").await;
    let id = bridge["id"].as_str().unwrap().to_string();

    // Cadena rota a propósito: id/name duplicados, refs inexistentes y una ref
    // a un campo que el origen no declara.
    let draft = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/acumula" },
        "blocks": [
            h.instance("paso-origen", "origen", &origen, json!({ "a": "$.fantasma.id" })),
            h.instance("paso-origen", "origen", &origen, json!({ "x": "$.nada.a" })),
            h.instance("paso-destino", "destino", &destino, json!({ "y": "$.origen.apellido" })),
        ]
    });
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;

    let validated = h
        .post(&format!("/api/v1/bridges/{id}/validate"), None)
        .await;
    assert_eq!(validated.status, StatusCode::OK, "{}", validated.body);
    let body = validated.json();
    assert_eq!(body["valid"], false, "{}", validated.body);
    let errors = body["errors"].as_array().expect("lista de errores");
    assert!(
        errors.len() >= 5,
        "acumula errores, no reporta solo el primero: {body:?}"
    );

    let fields: Vec<&str> = errors.iter().filter_map(|e| e["field"].as_str()).collect();
    for esperado in [
        "blocks[0].input.a",
        "blocks[1].id",
        "blocks[1].name",
        "blocks[1].input.x",
        "blocks[2].input.y",
    ] {
        assert!(
            fields.contains(&esperado),
            "falta {esperado} en {fields:?} — cuerpo: {}",
            validated.body
        );
    }
}

#[tokio::test]
async fn publicar_con_borrador_invalido_es_422_y_no_crea_version() {
    let Some(h) = begin().await else { return };

    let bridge = h.create_bridge("invalido-publicable").await;
    let id = bridge["id"].as_str().unwrap().to_string();

    // Sin trigger: la validación falla y la publicación se rechaza.
    let draft = json!({
        "blocks": [h.instance("paso", "paso", &(Uuid::now_v7(), Uuid::now_v7()), json!({}))]
    });
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;

    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    let problem = published.problem(StatusCode::UNPROCESSABLE_ENTITY);
    let errors = problem["errors"].as_array().expect("extensión errors");
    let fields: Vec<&str> = errors.iter().filter_map(|e| e["field"].as_str()).collect();
    assert!(
        fields.contains(&"trigger"),
        "el trigger faltante se reporta: {problem:?}"
    );

    // Nada se publicó.
    let bridge = h.get(&format!("/api/v1/bridges/{id}")).await.json();
    assert!(bridge["latestVersion"].is_null());
    let versions = h
        .get(&format!("/api/v1/bridges/{id}/versions"))
        .await
        .json();
    assert_eq!(versions["items"], json!([]));
}

#[tokio::test]
async fn un_puente_de_otro_workspace_es_404() {
    let Some(h) = begin().await else { return };

    let bridge = h.create_bridge("de-otro-workspace").await;
    let id = bridge["id"].as_str().unwrap().to_string();

    let (workspace_id, other_key) = h.extra_workspace().await;
    assert_ne!(workspace_id, h.workspace_id);

    let response = h
        .send(
            Method::GET,
            &format!("/api/v1/bridges/{id}"),
            Some(&other_key),
            None,
        )
        .await;
    response.problem(StatusCode::NOT_FOUND);

    let response = h
        .send(
            Method::POST,
            &format!("/api/v1/bridges/{id}/validate"),
            Some(&other_key),
            None,
        )
        .await;
    response.problem(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn borrar_un_puente_devuelve_204_y_lo_deja_inaccesible() {
    let Some(h) = begin().await else { return };

    let bridge = h.create_bridge("borrable").await;
    let id = bridge["id"].as_str().unwrap().to_string();

    let response = h.delete(&format!("/api/v1/bridges/{id}")).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", response.body);
    assert_eq!(response.body, "");

    h.get(&format!("/api/v1/bridges/{id}"))
        .await
        .problem(StatusCode::NOT_FOUND);

    let response = h.delete(&format!("/api/v1/bridges/{id}")).await;
    response.problem(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn el_trigger_incompleto_falla_en_validate_y_se_arregla_con_patch() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "trigger").await;
    let bridge = h.create_bridge("trigger-tardio").await;
    let id = bridge["id"].as_str().unwrap().to_string();

    // Borrador con bloques pero sin trigger: válido editar, inválido publicar.
    let draft = json!({ "blocks": [h.instance("paso", "origen", &origen, json!({}))] });
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;

    let validated = h
        .post(&format!("/api/v1/bridges/{id}/validate"), None)
        .await;
    assert_eq!(validated.status, StatusCode::OK);
    let body = validated.json();
    assert_eq!(body["valid"], false);
    assert_eq!(body["errors"][0]["field"], "trigger");

    // Se arregla con un PATCH del trigger y entonces publica.
    h.patch(
        &format!("/api/v1/bridges/{id}"),
        json!({ "trigger": { "type": "http", "method": "GET", "path": "/x/tardio" } }),
    )
    .await;

    let validated = h
        .post(&format!("/api/v1/bridges/{id}/validate"), None)
        .await;
    assert_eq!(validated.json()["valid"], true, "{}", validated.body);

    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    assert_eq!(published.json()["definition"]["trigger"]["type"], "http");
}
