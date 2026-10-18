//! Tests de integración de la biblioteca de bloques contra un Postgres **real**.
//!
//! Montan la misma aplicación axum que producción (`api::app`) y la ejercen a
//! través del router completo (auth Bearer incluida) con `tower::ServiceExt`.
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

    /// Campo `field` del primer elemento de la extensión `errors`.
    fn error_field(&self, index: usize) -> String {
        self.json()["errors"][index]["field"]
            .as_str()
            .unwrap_or_else(|| panic!("falta errors[{index}].field en: {}", self.body))
            .to_string()
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
            eprintln!("DATABASE_URL no está definida: se omiten los tests de integración de la API");
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
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'api-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces previos");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'api-test%'")
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

/// Inserta un workspace y una API key vinculada; devuelve `(key cruda, id de la key)`.
async fn seed_workspace(pool: &PgPool, workspace_id: Uuid) -> (String, Uuid) {
    let slug = format!("api-test-{workspace_id}");
    let key_id = Uuid::now_v7();
    let raw_key = format!("api-test-key-{workspace_id}");

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
    .bind(format!("api-test-{raw_key}"))
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

    async fn post(&self, uri: &str, body: Value) -> Resp {
        self.send(Method::POST, uri, Some(&self.key), Some(body)).await
    }

    async fn delete(&self, uri: &str) -> Resp {
        self.send(Method::DELETE, uri, Some(&self.key), None).await
    }

    /// Crea un bloque vía API y devuelve su JSON (asserta 201).
    async fn create_block(&self, slug: &str, block_type: &str) -> Value {
        let response = self
            .post(
                "/api/v1/blocks",
                json!({ "slug": slug, "name": slug, "blockType": block_type }),
            )
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
        response.json()
    }

    /// Publica una versión de un bloque y devuelve la respuesta cruda.
    async fn publish(&self, block_id: &str, body: Value) -> Resp {
        self.post(&format!("/api/v1/blocks/{block_id}/versions"), body)
            .await
    }

    /// Crea un workspace extra con su key; devuelve `(workspace_id, key cruda)`.
    async fn extra_workspace(&self) -> (Uuid, String) {
        let id = Uuid::now_v7();
        let (key, _) = seed_workspace(&self.pool, id).await;
        (id, key)
    }

    /// Crea un puente con una versión cuya definición incluye los `blockIds` dados.
    async fn insert_bridge_version(&self, workspace_id: Uuid, block_ids: &[Uuid]) -> Uuid {
        let bridge_id = Uuid::now_v7();
        let version_id = Uuid::now_v7();
        sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
            .bind(bridge_id)
            .bind(workspace_id)
            .bind(format!("puente-{bridge_id}"))
            .bind("Puente de test")
            .execute(&self.pool)
            .await
            .expect("bridge");

        let blocks: Vec<Value> = block_ids
            .iter()
            .map(|id| json!({ "id": Uuid::now_v7(), "blockId": id, "input": {}, "config": {} }))
            .collect();
        sqlx::query(
            "INSERT INTO bridge_versions (id, bridge_id, version, definition) VALUES ($1, $2, 1, $3)",
        )
        .bind(version_id)
        .bind(bridge_id)
        .bind(json!({ "trigger": { "type": "manual" }, "blocks": blocks }))
        .execute(&self.pool)
        .await
        .expect("bridge_version");

        version_id
    }

    async fn count_versions(&self, block_id: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM block_versions WHERE block_id = $1")
            .bind(block_id)
            .fetch_one(&self.pool)
            .await
            .expect("conteo de versiones")
    }

    async fn latest_version(&self, block_id: Uuid) -> Option<i32> {
        sqlx::query_scalar("SELECT latest_version FROM blocks WHERE id = $1")
            .bind(block_id)
            .fetch_one(&self.pool)
            .await
            .expect("latest_version")
    }

    async fn exists(&self, block_id: Uuid) -> bool {
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM blocks WHERE id = $1)")
            .bind(block_id)
            .fetch_one(&self.pool)
            .await
            .expect("existencia del bloque")
    }

    async fn set_created_at(&self, block_id: Uuid, created_at: DateTime<Utc>) {
        sqlx::query("UPDATE blocks SET created_at = $2 WHERE id = $1")
            .bind(block_id)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .expect("created_at de fixture");
    }
}

fn uuid_of(block: &Value) -> Uuid {
    Uuid::parse_str(block["id"].as_str().expect("id del bloque")).expect("uuid válido")
}

fn ts(second: u32) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("2026-01-01T00:00:{second:02}Z"))
        .expect("timestamp de fixture")
        .with_timezone(&Utc)
}

// ── auth → workspace ───────────────────────────────────────────────────

#[tokio::test]
async fn sin_bearer_la_biblioteca_responde_401_problem() {
    let Some(h) = begin().await else { return };

    let response = h
        .send(Method::GET, "/api/v1/blocks", None, None)
        .await;
    response.problem(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn una_key_sin_workspace_recibe_403_problem_en_la_biblioteca() {
    let Some(h) = begin().await else { return };

    // Key global (workspace_id NULL): válida para rutas sin workspace, no para
    // recursos de biblioteca.
    let raw_key = "api-test-key-sin-workspace";
    sqlx::query("INSERT INTO api_keys (id, name, key_hash) VALUES ($1, $2, $3)")
        .bind(Uuid::now_v7())
        .bind("api-test-sin-workspace")
        .bind(hash_key(raw_key))
        .execute(&h.pool)
        .await
        .expect("key sin workspace");

    let response = h
        .send(Method::GET, "/api/v1/blocks", Some(raw_key), None)
        .await;
    let problem = response.problem(StatusCode::FORBIDDEN);
    assert!(
        problem["detail"].as_str().unwrap().contains("workspace"),
        "el detalle debe explicar que falta el workspace: {problem}"
    );

    // `whoami` sigue funcionando: la key es válida, solo no tiene workspace.
    let whoami = h
        .send(Method::GET, "/api/v1/whoami", Some(raw_key), None)
        .await;
    assert_eq!(whoami.status, StatusCode::OK);
    assert!(whoami.json()["workspaceId"].is_null());
}

// ── CRUD de bloques ────────────────────────────────────────────────────

#[tokio::test]
async fn crea_un_bloque_y_lo_devuelve_en_el_detalle() {
    let Some(h) = begin().await else { return };

    let response = h
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

    assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
    let created = response.json();
    assert_eq!(created["slug"], "http-get");
    assert_eq!(created["name"], "HTTP GET");
    assert_eq!(created["blockType"], "http");
    assert_eq!(created["category"], "http");
    assert_eq!(created["description"], "Llama a una URL por GET");
    assert_eq!(created["icon"], "globe");
    assert_eq!(created["workspaceId"], h.workspace_id.to_string());
    assert!(created["latestVersion"].is_null());
    assert!(created["createdAt"].is_string());
    assert_eq!(
        Uuid::parse_str(created["id"].as_str().unwrap())
            .unwrap()
            .get_version_num(),
        7,
        "los ids son UUID v7"
    );

    let id = created["id"].as_str().unwrap();
    let detail = h.get(&format!("/api/v1/blocks/{id}")).await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.body);
    assert_eq!(detail.json(), created, "el detalle repite el recurso creado");

    // Los defaults de la tabla se aplican si el cliente no los manda.
    let minimal = h.create_block("minimo", "custom_logic").await;
    assert_eq!(minimal["category"], "general");
    assert_eq!(minimal["description"], "");
    assert_eq!(minimal["icon"], "box");
}

#[tokio::test]
async fn el_slug_es_unico_por_workspace_y_el_duplicado_es_409() {
    let Some(h) = begin().await else { return };

    h.create_block("duplicado", "custom_logic").await;

    let response = h
        .post(
            "/api/v1/blocks",
            json!({ "slug": "duplicado", "name": "otro", "blockType": "custom_logic" }),
        )
        .await;
    let problem = response.problem(StatusCode::CONFLICT);
    assert!(
        problem["detail"].as_str().unwrap().contains("duplicado"),
        "el detalle nombra el slug en conflicto: {problem}"
    );

    let list = h.get("/api/v1/blocks").await.json();
    assert_eq!(list["items"].as_array().unwrap().len(), 1, "no se creó el duplicado");

    // El mismo slug en otro workspace no es conflicto.
    let (_, other_key) = h.extra_workspace().await;
    let other = h
        .send(
            Method::POST,
            "/api/v1/blocks",
            Some(&other_key),
            Some(json!({ "slug": "duplicado", "name": "otro", "blockType": "custom_logic" })),
        )
        .await;
    assert_eq!(other.status, StatusCode::CREATED, "{}", other.body);
}

#[tokio::test]
async fn la_entrada_invalida_es_422_con_el_campo_señalado() {
    let Some(h) = begin().await else { return };

    let cases: [(Value, &str); 6] = [
        (
            json!({ "name": "sin slug", "blockType": "custom_logic" }),
            "slug",
        ),
        (
            json!({ "slug": "con espacio", "name": "x", "blockType": "custom_logic" }),
            "slug",
        ),
        (
            json!({ "slug": "Mayusculas", "name": "x", "blockType": "custom_logic" }),
            "slug",
        ),
        (
            json!({ "slug": "sin-nombre", "blockType": "custom_logic" }),
            "name",
        ),
        (
            json!({ "slug": "tipo-malo", "name": "x", "blockType": "http_get" }),
            "blockType",
        ),
        (
            json!({ "slug": "campo-extra", "name": "x", "blockType": "custom_logic", "raro": 1 }),
            "raro",
        ),
    ];

    for (body, field) in cases {
        let response = h.post("/api/v1/blocks", body.clone()).await;
        response.problem(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            response.error_field(0),
            field,
            "cuerpo: {body}; respuesta: {}",
            response.body
        );
    }

    assert_eq!(
        h.get("/api/v1/blocks").await.json()["items"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "ninguna entrada inválida creó filas"
    );
}

#[tokio::test]
async fn el_listado_pagina_por_cursor_opaco_sobre_created_at_e_id() {
    let Some(h) = begin().await else { return };

    // `created_at` fijo por fixture para que el orden sea determinista.
    for (index, slug) in ["uno", "dos", "tres"].iter().enumerate() {
        let block = h.create_block(slug, "custom_logic").await;
        h.set_created_at(uuid_of(&block), ts(index as u32 + 1)).await;
    }

    let first_page = h.get("/api/v1/blocks?limit=2").await;
    assert_eq!(first_page.status, StatusCode::OK, "{}", first_page.body);
    let page = first_page.json();
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["slug"], "tres");
    assert_eq!(items[1]["slug"], "dos");

    let cursor = page["nextCursor"].as_str().expect("hay página siguiente");
    assert!(
        !cursor.contains(&h.workspace_id.to_string()),
        "el cursor es opaco: {cursor}"
    );

    let second_page = h
        .get(&format!("/api/v1/blocks?limit=2&cursor={cursor}"))
        .await
        .json();
    let items = second_page["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["slug"], "uno");
    assert!(second_page["nextCursor"].is_null(), "se acabó el listado");

    // Un cursor corrupto es un 400 explícito, no un listado desde el principio.
    let corrupt = h.get("/api/v1/blocks?cursor=no-es-un-cursor").await;
    corrupt.problem(StatusCode::BAD_REQUEST);

    // Límite fuera de rango: también 400.
    let bad_limit = h.get("/api/v1/blocks?limit=0").await;
    bad_limit.problem(StatusCode::BAD_REQUEST);
    let too_big = h.get("/api/v1/blocks?limit=1000").await;
    too_big.problem(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn los_bloques_de_otro_workspace_no_son_visibles() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("privado", "custom_logic").await;
    let id = block["id"].as_str().unwrap().to_string();
    let (_, other_key) = h.extra_workspace().await;

    let detail = h
        .send(Method::GET, &format!("/api/v1/blocks/{id}"), Some(&other_key), None)
        .await;
    detail.problem(StatusCode::NOT_FOUND);

    let delete = h
        .send(
            Method::DELETE,
            &format!("/api/v1/blocks/{id}"),
            Some(&other_key),
            None,
        )
        .await;
    delete.problem(StatusCode::NOT_FOUND);

    let versions = h
        .send(
            Method::GET,
            &format!("/api/v1/blocks/{id}/versions"),
            Some(&other_key),
            None,
        )
        .await;
    versions.problem(StatusCode::NOT_FOUND);

    assert!(h.exists(uuid_of(&block)).await, "nada se borró");

    // Un id que no es UUID tampoco existe: 404, no 400 de axum.
    h.get("/api/v1/blocks/no-es-uuid").await.problem(StatusCode::NOT_FOUND);
    h.get("/api/v1/blocks/00000000-0000-0000-0000-000000000000")
        .await
        .problem(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn borra_un_bloque_sin_referencias_con_204() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("borrable", "custom_logic").await;
    let id = uuid_of(&block);
    let publicada = h
        .publish(
            &id.to_string(),
            json!({ "code": "export default (input) => input;" }),
        )
        .await;
    assert_eq!(publicada.status, StatusCode::CREATED, "{}", publicada.body);

    let response = h.delete(&format!("/api/v1/blocks/{id}")).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", response.body);
    assert_eq!(response.body, "");

    assert!(!h.exists(id).await, "el bloque se fue");
    assert_eq!(h.count_versions(id).await, 0, "las versiones caen en cascada");

    h.get(&format!("/api/v1/blocks/{id}"))
        .await
        .problem(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn un_bloque_referenciado_por_un_puente_no_se_borra_y_es_409() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("referenciado", "custom_logic").await;
    let id = uuid_of(&block);
    h.publish(
        &id.to_string(),
        json!({ "code": "export default (input) => input;" }),
    )
    .await;
    h.insert_bridge_version(h.workspace_id, &[id]).await;

    let response = h.delete(&format!("/api/v1/blocks/{id}")).await;
    let problem = response.problem(StatusCode::CONFLICT);
    assert!(
        problem["detail"].as_str().unwrap().contains("referenciado"),
        "el detalle explica el motivo: {problem}"
    );
    assert!(
        problem["bridgeSlug"].is_string(),
        "el problema nombra el puente que referencia: {problem}"
    );

    assert!(h.exists(id).await, "no se borró nada");
    assert_eq!(h.count_versions(id).await, 1, "tampoco las versiones");
}

#[tokio::test]
async fn una_referencia_de_otro_workspace_no_bloquea_el_borrado() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("ajeno", "custom_logic").await;
    let id = uuid_of(&block);
    let (other_workspace, _) = h.extra_workspace().await;
    // Una definición de otro workspace que apunta a nuestro bloque es una
    // referencia colgante: no debe impedir el borrado.
    h.insert_bridge_version(other_workspace, &[id]).await;

    let response = h.delete(&format!("/api/v1/blocks/{id}")).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", response.body);
    assert!(!h.exists(id).await);
}

// ── publicación de versiones ───────────────────────────────────────────

const BLOQUE_TS: &str = r#"
type Entrada = { userId: string };

export default function (input: Entrada, ctx: any) {
  const userId: string = input.userId.toUpperCase();
  return { userId, ahora: ctx === null ? null : undefined };
}
"#;

#[tokio::test]
async fn publica_la_primera_version_transpilada_y_actualiza_latest_version() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("transforma", "custom_logic").await;
    let id = uuid_of(&block);

    let response = h
        .publish(
            &id.to_string(),
            json!({
                "code": BLOQUE_TS,
                "configSchema": { "type": "object", "properties": { "sufijo": { "type": "string" } } },
                "inputSchema": {
                    "type": "object",
                    "required": ["userId"],
                    "properties": { "userId": { "type": "string" } }
                },
                "outputSchema": { "type": "object", "properties": { "userId": { "type": "string" } } },
                "ui": {
                    "title": "Transformar usuario",
                    "order": ["sufijo"],
                    "widget": { "sufijo": "text" },
                    "help": { "sufijo": "Se agrega al id" }
                }
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
    let version = response.json();
    assert_eq!(version["blockId"], id.to_string());
    assert_eq!(version["version"], 1);
    assert_eq!(version["ui"]["title"], "Transformar usuario");
    assert!(version["createdAt"].is_string());
    let code = version["code"].as_str().expect("código JS devuelto");
    assert!(code.contains("export default"), "el export default sobrevive: {code}");
    assert!(!code.contains(": string"), "el JS guardado no tiene tipos: {code}");
    assert!(!code.contains("type Entrada"), "el JS guardado no tiene tipos: {code}");
    assert_eq!(h.latest_version(id).await, Some(1));

    // Segunda publicación: version=2 y latest_version avanza.
    let second = h
        .publish(
            &id.to_string(),
            json!({ "code": "export default (input: any) => input;" }),
        )
        .await;
    assert_eq!(second.status, StatusCode::CREATED, "{}", second.body);
    assert_eq!(second.json()["version"], 2);
    assert_eq!(h.latest_version(id).await, Some(2));

    // Inmutabilidad: la v1 conserva su código y su ui.
    let persisted: (i32, String, Value) = sqlx::query_as(
        "SELECT version, code, ui FROM block_versions WHERE block_id = $1 ORDER BY version LIMIT 1",
    )
    .bind(id)
    .fetch_one(&h.pool)
    .await
    .expect("v1 persistida");
    assert_eq!(persisted.0, 1);
    assert_eq!(persisted.1, code);
    assert_eq!(persisted.2["title"], "Transformar usuario");

    // Los schemas por defecto quedan como objeto vacío.
    let empty = h
        .publish(
            &id.to_string(),
            json!({ "code": "export default (input: any) => input;" }),
        )
        .await
        .json();
    assert_eq!(empty["configSchema"], json!({}));
    assert_eq!(empty["inputSchema"], json!({}));
    assert_eq!(empty["outputSchema"], json!({}));
    assert_eq!(empty["ui"], json!({}));
}

#[tokio::test]
async fn el_js_publicado_es_el_que_ejecuta_el_sandbox() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("ejecutable", "custom_logic").await;
    let id = uuid_of(&block);
    let version = h
        .publish(&id.to_string(), json!({ "code": BLOQUE_TS }))
        .await
        .json();

    let code: String = sqlx::query_scalar("SELECT code FROM block_versions WHERE id = $1")
        .bind(Uuid::parse_str(version["id"].as_str().unwrap()).unwrap())
        .fetch_one(&h.pool)
        .await
        .expect("código persistido");

    let salida = sandbox::run_block(
        &code,
        json!({ "userId": "ana" }),
        sandbox::SandboxConfig::default(),
    )
    .await
    .expect("el JS publicado se carga y ejecuta");
    assert_eq!(salida["userId"], "ANA");
}

#[tokio::test]
async fn un_error_de_sintaxis_es_422_y_no_crea_version() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("roto", "custom_logic").await;
    let id = uuid_of(&block);

    let response = h
        .publish(
            &id.to_string(),
            json!({ "code": "export default function () {\n  const roto: = ;\n}" }),
        )
        .await;
    let problem = response.problem(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.error_field(0), "code");
    assert_eq!(
        problem["errors"][0]["line"], 2,
        "el 422 reporta la línea del fallo: {problem}"
    );

    assert_eq!(h.count_versions(id).await, 0, "no se creó versión");
    assert!(h.latest_version(id).await.is_none());
}

#[tokio::test]
async fn un_bloque_sin_export_default_es_422_y_no_crea_version() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("sin-default", "custom_logic").await;
    let id = uuid_of(&block);

    let response = h
        .publish(
            &id.to_string(),
            json!({ "code": "export function transforma(input: any) { return input; }" }),
        )
        .await;
    let problem = response.problem(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.error_field(0), "code");
    assert!(
        problem["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("export default"),
        "{problem}"
    );
    assert_eq!(h.count_versions(id).await, 0);
}

#[tokio::test]
async fn un_schema_fuera_del_subconjunto_es_422_y_no_crea_version() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("schema-malo", "custom_logic").await;
    let id = uuid_of(&block);
    let code = json!("export default (input: any) => input;");

    for field in ["configSchema", "inputSchema", "outputSchema"] {
        let mut body = json!({ "code": code });
        // `pattern` está fuera del subconjunto estricto de design.md.
        body[field] = json!({ "type": "object", "properties": { "x": { "pattern": "^a" } } });

        let response = h.publish(&id.to_string(), body).await;
        let problem = response.problem(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response.error_field(0), field, "{problem}");
        assert!(
            problem["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("pattern"),
            "el detalle nombra la keyword rechazada: {problem}"
        );
    }

    // Un schema que no es objeto JSON también se rechaza.
    let response = h
        .publish(
            &id.to_string(),
            json!({ "code": code, "ui": {}, "inputSchema": "object" }),
        )
        .await;
    response.problem(StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(h.count_versions(id).await, 0);
}

#[tokio::test]
async fn un_ui_fuera_del_contrato_es_422_y_no_crea_version() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("ui-malo", "custom_logic").await;
    let id = uuid_of(&block);
    let code = json!("export default (input: any) => input;");

    for (ui, needle) in [
        (json!({ "colores": [] }), "colores"),
        (json!({ "title": 7 }), "title"),
        (json!({ "widget": { "url": "Ref-Template" } }), "widget"),
        (json!({ "order": ["a", "a"] }), "order"),
    ] {
        let response = h
            .publish(&id.to_string(), json!({ "code": code, "ui": ui }))
            .await;
        let problem = response.problem(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response.error_field(0), "ui");
        assert!(
            problem["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains(needle),
            "el detalle nombra el problema ({needle}): {problem}"
        );
    }

    // El widget reservado para strings interpolables sí se admite.
    let ok = h
        .publish(
            &id.to_string(),
            json!({ "code": code, "ui": { "widget": { "url": "ref-template" } } }),
        )
        .await;
    assert_eq!(ok.status, StatusCode::CREATED, "{}", ok.body);
    assert_eq!(h.count_versions(id).await, 1);
}

#[tokio::test]
async fn un_bloque_http_no_publica_codigo() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("builtin-get", "http").await;
    let id = uuid_of(&block);

    let response = h
        .publish(&id.to_string(), json!({ "code": "export default () => 1;" }))
        .await;
    let problem = response.problem(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.error_field(0), "blockType");
    assert!(
        problem["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("http"),
        "el error explica que el built-in http no lleva código: {problem}"
    );
    assert_eq!(h.count_versions(id).await, 0);
}

#[tokio::test]
async fn publicar_en_un_bloque_inexistente_es_404() {
    let Some(h) = begin().await else { return };

    h.publish(
        "no-es-uuid",
        json!({ "code": "export default () => 1;" }),
    )
    .await
    .problem(StatusCode::NOT_FOUND);

    h.publish(
        &Uuid::now_v7().to_string(),
        json!({ "code": "export default () => 1;" }),
    )
    .await
    .problem(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn el_cuerpo_de_publicacion_se_valida_antes_de_tocar_la_base() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("cuerpo-malo", "custom_logic").await;
    let id = uuid_of(&block);

    // Sin `code` y con un campo desconocido: 422 Problem Details (no el 400 de
    // texto plano de axum).
    h.publish(&id.to_string(), json!({ "configSchema": {} }))
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY);
    h.publish(&id.to_string(), json!({ "code": "export default () => 1;", "extra": 1 }))
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY);
    h.publish(&id.to_string(), json!({ "code": 42 }))
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(h.count_versions(id).await, 0);
}

#[tokio::test]
async fn las_versiones_se_listan_de_mas_nueva_a_mas_vieja_con_cursor() {
    let Some(h) = begin().await else { return };

    let block = h.create_block("versionado", "custom_logic").await;
    let id = uuid_of(&block);
    for _ in 0..3 {
        let response = h
            .publish(&id.to_string(), json!({ "code": "export default (input: any) => input;" }))
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
    }

    let first_page = h
        .get(&format!("/api/v1/blocks/{id}/versions?limit=2"))
        .await;
    assert_eq!(first_page.status, StatusCode::OK, "{}", first_page.body);
    let page = first_page.json();
    let versions: Vec<i64> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["version"].as_i64().unwrap())
        .collect();
    assert_eq!(versions, vec![3, 2]);

    let cursor = page["nextCursor"].as_str().expect("hay más versiones");
    let second = h
        .get(&format!(
            "/api/v1/blocks/{id}/versions?limit=2&cursor={cursor}"
        ))
        .await
        .json();
    let versions: Vec<i64> = second["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["version"].as_i64().unwrap())
        .collect();
    assert_eq!(versions, vec![1]);
    assert!(second["nextCursor"].is_null());

    h.get(&format!("/api/v1/blocks/{id}/versions?cursor=malo"))
        .await
        .problem(StatusCode::BAD_REQUEST);
}
