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

    /// Crea un bloque built-in `http` y publica una versión SIN código
    /// (schemas solos): es lo que instancia un built-in o un `switch`.
    async fn published_builtin(&self, slug: &str) -> (Uuid, Uuid) {
        let response = self
            .post(
                "/api/v1/blocks",
                Some(json!({ "slug": slug, "name": slug, "blockType": "http" })),
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
                Some(json!({ "inputSchema": json!({}), "outputSchema": json!({}) })),
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

// ── enrutado condicional y trigger cron (MVP2) ─────────────────────────

/// Instancia con config y `when` opcionales (para puentes condicionales).
fn instancia(
    id: &str,
    name: &str,
    block: &(Uuid, Uuid),
    input: Value,
    config: Value,
    when: Value,
) -> Value {
    let mut body = json!({
        "id": id,
        "name": name,
        "blockId": block.0.to_string(),
        "blockVersionId": block.1.to_string(),
        "input": input,
        "config": config,
    });
    if !when.is_null() {
        body["when"] = when;
    }
    body
}

#[tokio::test]
async fn publica_un_puente_condicional_con_switch_valido_y_congela_el_snapshot() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "cond").await;
    let builtin = h.published_builtin("decisor-cond").await;
    let vip = h
        .published_block(
            "vip-cond",
            json!({ "type": "object", "properties": { "total": { "type": "integer" } } }),
            json!({}),
        )
        .await;
    let normal = bloque_destino(&h, "cond").await;

    let draft = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/condicional" },
        "blocks": [
            h.instance("paso-origen", "origen", &origen, json!({})),
            instancia(
                "paso-switch",
                "decisor",
                &builtin,
                json!({}),
                json!({
                    "routes": [
                        { "when": { "gte": ["$.origen.total", 10] }, "to": "paso-vip" },
                        { "when": { "exists": "$.origen.cliente" }, "to": "paso-normal" }
                    ],
                    "default": "paso-normal"
                }),
                json!(null),
            ),
            instancia("paso-vip", "vip", &vip, json!({ "total": "$.origen.total" }), json!({}), json!(null)),
            instancia("paso-normal", "normal", &normal, json!({ "total": "$.origen.total" }), json!({}), json!(null)),
        ]
    });
    let id = h.create_bridge("condicional-ok").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.patch(&format!("/api/v1/bridges/{id}"), draft.clone())
        .await;

    let validated = h
        .post(&format!("/api/v1/bridges/{id}/validate"), None)
        .await;
    assert_eq!(validated.json()["valid"], true, "{}", validated.body);

    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);

    // El snapshot congela `when`, `routes` y `default`.
    let version = published.json();
    let blocks = version["definition"]["blocks"].as_array().expect("bloques");
    assert_eq!(blocks[1]["config"]["default"], json!("paso-normal"));
    assert_eq!(
        blocks[1]["config"]["routes"][0],
        json!({ "when": { "gte": ["$.origen.total", 10] }, "to": "paso-vip" })
    );
}

#[tokio::test]
async fn una_ruta_del_switch_que_salta_hacia_atras_bloquea_la_publicacion() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "atras").await;
    let builtin = h.published_builtin("decisor-atras").await;

    let draft = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/ciclo" },
        "blocks": [
            h.instance("paso-origen", "origen", &origen, json!({})),
            instancia(
                "paso-switch",
                "decisor",
                &builtin,
                json!({}),
                json!({ "routes": [{ "when": { "exists": "$.origen.total" }, "to": "paso-origen" }] }),
                json!(null),
            ),
        ]
    });
    let id = h.create_bridge("condicional-ciclo").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;

    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    let problem = published.problem(StatusCode::UNPROCESSABLE_ENTITY);
    let errors = problem["errors"].as_array().expect("errors");
    assert!(
        errors.iter().any(|e| {
            e["field"] == json!("blocks[1].config.routes[0]")
                && e["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("salta hacia atrás")
        }),
        "{problem:?}"
    );
}

#[tokio::test]
async fn un_when_con_codigo_arbitrario_se_rechaza_como_no_declarativo() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "js").await;
    let destino = bloque_destino(&h, "js").await;

    for cuando in [
        json!("$.origen.total > 10"),
        json!({ "js": "function (v) { return v > 10 }" }),
        json!({ "==": ["$.origen.total", 10] }),
    ] {
        let draft = json!({
            "trigger": { "type": "http", "method": "POST", "path": "/x/no-js" },
            "blocks": [
                h.instance("paso-origen", "origen", &origen, json!({})),
                instancia(
                    "paso-destino",
                    "destino",
                    &destino,
                    json!({ "total": "$.origen.total" }),
                    json!({}),
                    cuando.clone(),
                ),
            ]
        });
        let id = h.create_bridge("condicional-js").await["id"]
            .as_str()
            .unwrap()
            .to_string();
        h.patch(&format!("/api/v1/bridges/{id}"), draft).await;

        let published = h
            .post(&format!("/api/v1/bridges/{id}/versions"), None)
            .await;
        let problem = published.problem(StatusCode::UNPROCESSABLE_ENTITY);
        let errors = problem["errors"].as_array().expect("errors");
        assert!(
            errors.iter().any(|e| {
                e["field"] == json!("blocks[1].when")
                    && e["message"].as_str().unwrap_or("").contains("declarativas")
            }),
            "{cuando}: {problem:?}"
        );
        // Limpia para el siguiente intento.
        h.delete(&format!("/api/v1/bridges/{id}")).await;
    }
}

#[tokio::test]
async fn el_trigger_cron_se_valida_al_publicar_y_se_congela_en_el_snapshot() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "cron").await;

    // Válido: publica y congela schedule + timezone.
    let draft = json!({
        "trigger": { "type": "cron", "schedule": "0 9 * * 1-5", "timezone": "Europe/Madrid" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    let id = h.create_bridge("cron-ok").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    let version = published.json();
    assert_eq!(
        version["definition"]["trigger"]["schedule"],
        json!("0 9 * * 1-5")
    );
    assert_eq!(
        version["definition"]["trigger"]["timezone"],
        json!("Europe/Madrid")
    );
    h.delete(&format!("/api/v1/bridges/{id}")).await;

    // Schedule que no es cron de 5 campos.
    let draft = json!({
        "trigger": { "type": "cron", "schedule": "no-es-cron", "timezone": "UTC" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    let id = h.create_bridge("cron-malo").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    let problem = published.problem(StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        problem["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .any(|e| e["field"] == json!("trigger.schedule")),
        "{problem:?}"
    );
    h.delete(&format!("/api/v1/bridges/{id}")).await;

    // Timezone IANA desconocida.
    let draft = json!({
        "trigger": { "type": "cron", "schedule": "*/5 * * * *", "timezone": "Marte/Olympus" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    let id = h.create_bridge("cron-tz").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    let problem = published.problem(StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        problem["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .any(|e| e["field"] == json!("trigger.timezone")),
        "{problem:?}"
    );

    // Cron sin schedule ni timezone.
    let draft = json!({
        "trigger": { "type": "cron" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    let id = h.create_bridge("cron-vacio").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    let problem = published.problem(StatusCode::UNPROCESSABLE_ENTITY);
    let fields: Vec<&str> = problem["errors"]
        .as_array()
        .expect("errors")
        .iter()
        .filter_map(|e| e["field"].as_str())
        .collect();
    assert!(fields.contains(&"trigger.schedule"), "{problem:?}");
    assert!(fields.contains(&"trigger.timezone"), "{problem:?}");
}

// ── webhook de resultado (MVP2, task 4.1) ──────────────────────────────

#[tokio::test]
async fn publica_puente_con_webhook_https_congela_la_url_y_expone_el_secreto_por_admin() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "wh").await;
    let draft = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/con-hook" },
        "webhook": { "url": "https://receptor.example.com/hook" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    let id = h.create_bridge("webhook-ok").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;

    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    let version = published.json();
    assert_eq!(
        version["definition"]["webhook"]["url"],
        json!("https://receptor.example.com/hook"),
        "el snapshot congela la URL"
    );

    // El secreto solo sale por el plano admin: 64 hex (256 bits).
    let secreto_resp = h.get(&format!("/api/v1/bridges/{id}/webhook-secret")).await;
    assert_eq!(secreto_resp.status, StatusCode::OK, "{}", secreto_resp.body);
    let secreto_json = secreto_resp.json();
    let secreto = secreto_json["secret"].as_str().expect("secret").to_string();
    assert_eq!(secreto.len(), 64, "256 bits en hex: {secreto}");
    assert!(secreto.chars().all(|c| c.is_ascii_hexdigit()));

    // Estable entre publicaciones (regenerarlo rompería a los consumidores).
    h.post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    let secreto2 = h.get(&format!("/api/v1/bridges/{id}/webhook-secret")).await;
    assert_eq!(secreto2.status, StatusCode::OK);
    assert_eq!(
        secreto2.json()["secret"],
        json!(secreto),
        "el secreto no cambia entre versiones"
    );

    // Sin auth de administración, el secreto no se expone: 401 Problem.
    let response = h
        .app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/api/v1/bridges/{id}/webhook-secret"))
                .body(axum::body::Body::empty())
                .expect("petición válida"),
        )
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn una_webhook_sin_https_bloquea_la_publicacion() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "whhttp").await;
    for url in [
        "http://receptor.example.com/hook",
        "ftp://receptor.example.com",
    ] {
        let draft = json!({
            "trigger": { "type": "http", "method": "POST", "path": "/x/hook-inseguro" },
            "webhook": { "url": url },
            "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
        });
        let id = h.create_bridge("webhook-inseguro").await["id"]
            .as_str()
            .unwrap()
            .to_string();
        h.patch(&format!("/api/v1/bridges/{id}"), draft).await;

        let published = h
            .post(&format!("/api/v1/bridges/{id}/versions"), None)
            .await;
        let problem = published.problem(StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            problem["errors"]
                .as_array()
                .expect("errors")
                .iter()
                .any(|e| e["field"] == json!("webhook.url")),
            "{url}: {problem:?}"
        );
        h.delete(&format!("/api/v1/bridges/{id}")).await;
    }
}

#[tokio::test]
async fn el_secreto_de_un_puente_sin_webhook_es_404() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "whnulo").await;
    let draft = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/sin-hook" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    let id = h.create_bridge("sin-webhook").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    h.post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;

    let respuesta = h.get(&format!("/api/v1/bridges/{id}/webhook-secret")).await;
    respuesta.problem(StatusCode::NOT_FOUND);
}

// ── disparo manual y cron fuera de /x (MVP2, tasks 5.4-5.5) ────────────

#[tokio::test]
async fn run_encola_un_disparo_manual_inmediato_de_la_version_publicada() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "run").await;
    let draft = json!({
        "trigger": { "type": "cron", "schedule": "0 9 * * 1-5", "timezone": "Europe/Madrid" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    let id = h.create_bridge("runnable").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    h.post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;

    let response = h.post(&format!("/api/v1/bridges/{id}/run"), None).await;
    assert_eq!(response.status, StatusCode::ACCEPTED, "{}", response.body);
    let sobre = response.json();
    let execution_id = sobre["executionId"].as_str().expect("executionId");

    // La ejecución nace `manual`, `pending`, con run_at inmediato.
    let (tipo, estado): (String, String) =
        sqlx::query_as("SELECT trigger_type, status FROM executions WHERE id = $1")
            .bind(execution_id.parse::<Uuid>().expect("uuid"))
            .fetch_one(&h.pool)
            .await
            .expect("ejecución");
    assert_eq!((tipo.as_str(), estado.as_str()), ("manual", "pending"));
}

#[tokio::test]
async fn run_sin_version_publicada_es_409() {
    let Some(h) = begin().await else { return };

    let bridge = h.create_bridge("sin-publicar-run").await;
    let id = bridge["id"].as_str().unwrap().to_string();

    let response = h.post(&format!("/api/v1/bridges/{id}/run"), None).await;
    let problem = response.problem(StatusCode::CONFLICT);
    assert!(
        problem["detail"]
            .as_str()
            .unwrap_or("")
            .contains("versión publicada"),
        "{problem:?}"
    );
}

#[tokio::test]
async fn un_puente_cron_publicado_no_se_expone_en_x() {
    let Some(h) = begin().await else { return };

    let origen = bloque_origen(&h, "cronx").await;
    let draft = json!({
        "trigger": { "type": "cron", "schedule": "*/5 * * * *", "timezone": "UTC" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    let id = h.create_bridge("solo-cron").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);

    // Ni POST ni GET: el cron no registra ruta de invocación.
    let post = h
        .send(
            Method::POST,
            "/x/solo-cron",
            None,
            Some(json!({ "cualquier": "cosa" })),
        )
        .await;
    post.problem(StatusCode::NOT_FOUND);

    let get = h.send(Method::GET, "/x/solo-cron", None, None).await;
    get.problem(StatusCode::NOT_FOUND);
}

// ── validación de mask (MVP3, task 2.1) ────────────────────────────────

#[tokio::test]
async fn mask_se_valida_con_raices_trigger_o_bloques_existentes() {
    let Some(h) = begin().await else { return };
    let origen = bloque_origen(&h, "maskv").await;

    let bridge = h.create_bridge("mask-valida").await;
    let id = bridge["id"].as_str().unwrap().to_string();

    // Válido: trigger y un bloque existente de la cadena.
    let draft = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/maskv" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))],
        "mask": ["$.trigger.password", "$.origen.total"]
    });
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    let validated = h
        .post(&format!("/api/v1/bridges/{id}/validate"), None)
        .await;
    assert_eq!(validated.status, StatusCode::OK, "{}", validated.body);
    assert_eq!(validated.json()["valid"], true, "{}", validated.body);

    // Inválido: raíz desconocida, ruta sin `$.` y mask que no es array.
    for (path, mask) in [
        ("mask.0", json!(["$.bloque-inexistente.total"])),
        ("mask.0", json!(["trigger.password"])),
        ("mask", json!("$.trigger.password")),
    ] {
        let draft = json!({
            "trigger": { "type": "http", "method": "POST", "path": "/x/maskv" },
            "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))],
            "mask": mask
        });
        h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
        let validated = h
            .post(&format!("/api/v1/bridges/{id}/validate"), None)
            .await;
        assert_eq!(validated.status, StatusCode::OK, "{}", validated.body);
        assert_eq!(
            validated.json()["valid"],
            false,
            "mask {mask} debió ser inválida: {}",
            validated.body
        );
        assert!(
            validated.body.contains("mask"),
            "el error debe señalar la regla ofensiva ({path}): {}",
            validated.body
        );
    }
}

#[tokio::test]
async fn trigger_event_exige_topic_con_formato_slug() {
    let Some(h) = begin().await else { return };
    let origen = bloque_origen(&h, "evento").await;
    let bridge = h.create_bridge("evento-topic").await;
    let id = bridge["id"].as_str().unwrap().to_string();

    // Sin topic: issue de validación.
    let draft = json!({
        "trigger": { "type": "event" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    let validated = h
        .post(&format!("/api/v1/bridges/{id}/validate"), None)
        .await;
    assert_eq!(validated.json()["valid"], false, "{}", validated.body);
    assert!(
        validated.body.contains("trigger.topic"),
        "{}",
        validated.body
    );

    // Topic con formato inválido: issue que señala el formato.
    let draft = json!({
        "trigger": { "type": "event", "topic": "Pedido Creado!!" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    let validated = h
        .post(&format!("/api/v1/bridges/{id}/validate"), None)
        .await;
    assert_eq!(validated.json()["valid"], false, "{}", validated.body);
    assert!(
        validated.body.contains("no es un topic válido"),
        "{}",
        validated.body
    );

    // Topic válido: la publicación congela el trigger completo.
    let draft = json!({
        "trigger": { "type": "event", "topic": "pedido.creado" },
        "blocks": [h.instance("paso-origen", "origen", &origen, json!({}))]
    });
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;
    let validated = h
        .post(&format!("/api/v1/bridges/{id}/validate"), None)
        .await;
    assert_eq!(validated.json()["valid"], true, "{}", validated.body);
    let published = h
        .post(&format!("/api/v1/bridges/{id}/versions"), None)
        .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    assert_eq!(
        published.json()["definition"]["trigger"]["topic"],
        json!("pedido.creado"),
        "el topic queda congelado en el snapshot"
    );
}

// ── dry-run (MVP3, tasks 4.1-4.2) ──────────────────────────────────────

#[tokio::test]
async fn el_dry_run_corre_el_borrador_sin_publicar_ni_dejar_rastro() {
    let Some(h) = begin().await else { return };
    let origen = bloque_origen(&h, "dry").await;
    let destino = bloque_destino(&h, "dry").await;

    let bridge = h.create_bridge("ensayo").await;
    let id = bridge["id"].as_str().unwrap().to_string();
    let draft = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/ensayo" },
        "blocks": [
            h.instance("b1", "origen", &origen, json!({ "total": 5 })),
            h.instance("b2", "destino", &destino, json!({ "total": "$.origen.total" })),
        ]
    });
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;

    // Cero ejecuciones antes (el puente nunca se publicó).
    let antes = h.get("/api/v1/executions").await;
    assert_eq!(antes.status, StatusCode::OK, "{}", antes.body);
    assert_eq!(antes.json()["items"].as_array().map(Vec::len), Some(0));

    // Dry-run del borrador SIN versión publicada: trace en la respuesta.
    let ensayo = h
        .post(&format!("/api/v1/bridges/{id}/dry-run"), Some(json!({})))
        .await;
    assert_eq!(ensayo.status, StatusCode::OK, "{}", ensayo.body);
    let cuerpo = ensayo.json();
    assert_eq!(cuerpo["status"], json!("succeeded"), "{}", cuerpo);
    assert_eq!(cuerpo["output"], json!({ "total": 5 }));

    let steps = cuerpo["steps"].as_array().expect("steps");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["blockName"], json!("origen"));
    assert_eq!(steps[0]["output"], json!({ "total": 5 }));
    assert!(steps[0]["durationMs"].as_i64().is_some());
    assert_eq!(steps[1]["blockName"], json!("destino"));
    assert_eq!(
        steps[1]["input"],
        json!({ "total": 5 }),
        "el segundo bloque consumió la salida real del primero"
    );

    // El dry-run no deja rastro: el listado sigue vacío.
    let despues = h.get("/api/v1/executions").await;
    assert_eq!(despues.json()["items"].as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn el_mock_salta_el_bloque_y_alimenta_la_cadena_mixta() {
    let Some(h) = begin().await else { return };
    let origen = bloque_origen(&h, "drymock").await;
    let destino = bloque_destino(&h, "drymock").await;

    let bridge = h.create_bridge("ensayo-mock").await;
    let id = bridge["id"].as_str().unwrap().to_string();
    let draft = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/ensayo-mock" },
        "blocks": [
            h.instance("b1", "origen", &origen, json!({ "total": 5 })),
            h.instance("b2", "destino", &destino, json!({ "total": "$.origen.total" })),
        ]
    });
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;

    // Solo el primer bloque va mockeado: el segundo corre su código real
    // consumiendo la salida del mock.
    let ensayo = h
        .post(
            &format!("/api/v1/bridges/{id}/dry-run"),
            Some(json!({ "mocks": { "origen": { "output": { "total": 99 } } } })),
        )
        .await;
    assert_eq!(ensayo.status, StatusCode::OK, "{}", ensayo.body);
    let cuerpo = ensayo.json();
    let steps = cuerpo["steps"].as_array().expect("steps");
    assert_eq!(
        steps[0]["output"],
        json!({ "total": 99 }),
        "el mock alimenta el contexto sin invocar el bloque"
    );
    assert_eq!(steps[1]["input"], json!({ "total": 99 }));
    assert_eq!(cuerpo["output"], json!({ "total": 99 }));

    // Mock por ID de instancia también.
    let ensayo = h
        .post(
            &format!("/api/v1/bridges/{id}/dry-run"),
            Some(json!({ "mocks": { "b2": { "output": { "total": 1, "extra": true } } } })),
        )
        .await;
    assert_eq!(ensayo.status, StatusCode::OK, "{}", ensayo.body);
    assert_eq!(
        ensayo.json()["output"],
        json!({ "total": 1, "extra": true }),
        "el segundo bloque mockeado fija la salida final"
    );

    // Mock mal formado (sin output): 422.
    let ensayo = h
        .post(
            &format!("/api/v1/bridges/{id}/dry-run"),
            Some(json!({ "mocks": { "origen": {} } })),
        )
        .await;
    assert_eq!(
        ensayo.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        ensayo.body
    );
}

#[tokio::test]
async fn el_dry_run_de_un_borrador_invalido_es_422() {
    let Some(h) = begin().await else { return };
    let origen = bloque_origen(&h, "drymalo").await;

    let bridge = h.create_bridge("ensayo-malo").await;
    let id = bridge["id"].as_str().unwrap().to_string();
    // Ref rota: el destino pide un campo que el origen no produce.
    let destino_malo = h
        .published_block(
            "destino-malo-dry",
            json!({ "type": "object", "properties": { "total": { "type": "integer" } } }),
            json!({}),
        )
        .await;
    let draft = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/ensayo-malo" },
        "blocks": [
            h.instance("b1", "origen", &origen, json!({})),
            h.instance("b2", "destino", &destino_malo, json!({ "total": "$.origen.inexistente" })),
        ]
    });
    h.patch(&format!("/api/v1/bridges/{id}"), draft).await;

    let ensayo = h
        .post(&format!("/api/v1/bridges/{id}/dry-run"), Some(json!({})))
        .await;
    assert_eq!(
        ensayo.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        ensayo.body
    );
    assert!(ensayo.body.contains("validación"), "{}", ensayo.body);
}

// ── definición congelada de una versión (MVP5, spec `bridge-version-access`) ──

#[tokio::test]
async fn la_definicion_congelada_de_una_version_se_recupera_exacta() {
    let Some(h) = begin().await else { return };
    let bridge_id = Uuid::now_v7();
    sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
        .bind(bridge_id)
        .bind(h.workspace_id)
        .bind("congelada")
        .bind("Puente congelada")
        .execute(&h.pool)
        .await
        .expect("bridge");
    let definition = json!({
        "trigger": { "type": "http", "method": "POST", "path": "/x/congelada" },
        "blocks": [{ "id": "b1", "name": "eco", "input": {"marca": "v1"} }]
    });
    sqlx::query(
        "INSERT INTO bridge_versions (id, bridge_id, version, definition) \
         VALUES ($1, $2, 1, $3)",
    )
    .bind(Uuid::now_v7())
    .bind(bridge_id)
    .bind(&definition)
    .execute(&h.pool)
    .await
    .expect("versión 1");

    let base = format!("/api/v1/bridges/{bridge_id}/versions");

    // Recuperación exacta del snapshot.
    let r = h
        .send(Method::GET, &format!("{base}/1"), Some(&h.key), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let cuerpo = r.json();
    assert_eq!(cuerpo["version"], json!(1));
    assert_eq!(cuerpo["definition"], definition, "definición idéntica");
    assert!(cuerpo["id"].is_string());
    assert!(cuerpo["createdAt"].is_string());

    // Versión inexistente → 404; versión no entera → 400.
    let r = h
        .send(Method::GET, &format!("{base}/9"), Some(&h.key), None)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    let r = h
        .send(Method::GET, &format!("{base}/abc"), Some(&h.key), None)
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert!(r.body.contains("entero"), "{}", r.body);
}

// ── rollback por re-publicación: el pasado queda intacto (MVP5, 4.3) ──

#[tokio::test]
async fn el_rollback_republica_sin_tocar_versiones_ni_ejecuciones_previas() {
    let Some(h) = begin().await else { return };
    let (block_id, version_id) = h
        .published_block(
            "rollback-eco",
            json!({ "type": "object" }),
            json!({ "type": "object" }),
        )
        .await;
    let instancia = json!({
        "id": "b1", "name": "eco",
        "blockId": block_id, "blockVersionId": version_id,
        "input": {}, "config": {}
    });

    // Puente con borrador y publicación de la v1.
    let bridge_id = Uuid::now_v7();
    sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name, draft) VALUES ($1, $2, $3, $4, $5)")
        .bind(bridge_id)
        .bind(h.workspace_id)
        .bind("rollback")
        .bind("Puente rollback")
        .bind(json!({ "trigger": { "type": "http", "method": "POST", "path": "/x/rollback" }, "blocks": [instancia] }))
        .execute(&h.pool)
        .await
        .expect("bridge");
    let v1 = h
        .post(&format!("/api/v1/bridges/{bridge_id}/versions"), None)
        .await;
    assert_eq!(v1.status, StatusCode::CREATED, "{}", v1.body);
    let v1 = v1.json();
    assert_eq!(v1["version"], json!(1));
    let definicion_v1 = v1["definition"].clone();
    let bridge_version_v1: Uuid = v1["id"].as_str().unwrap().parse().unwrap();

    // Ejecución real sobre la v1.
    let ejecucion = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO executions (id, workspace_id, bridge_id, bridge_version_id, \
         trigger_type, status, created_at) \
         VALUES ($1, $2, $3, $4, 'manual', 'succeeded', now())",
    )
    .bind(ejecucion)
    .bind(h.workspace_id)
    .bind(bridge_id)
    .bind(bridge_version_v1)
    .execute(&h.pool)
    .await
    .expect("ejecución");

    // v2 con la cadena cambiada (lo que sería «romper» el puente).
    let mut instancia_v2 = instancia.clone();
    instancia_v2["name"] = json!("eco-v2");
    let parche = h
        .patch(
            &format!("/api/v1/bridges/{bridge_id}"),
            json!({ "blocks": [instancia_v2] }),
        )
        .await;
    assert_eq!(parche.status, StatusCode::OK);
    let v2 = h
        .post(&format!("/api/v1/bridges/{bridge_id}/versions"), None)
        .await;
    assert_eq!(v2.status, StatusCode::CREATED, "{}", v2.body);
    assert_eq!(v2.json()["version"], json!(2));

    // ROLLBACK (lo que orquesta la consola): definición de la v1 → PATCH del
    // borrador sección por sección → publicar.
    for clave in [
        "trigger",
        "inputSchema",
        "blocks",
        "mask",
        "emit",
        "webhook",
    ] {
        if let Some(seccion) = definicion_v1.get(clave) {
            let respuesta = h
                .patch(
                    &format!("/api/v1/bridges/{bridge_id}"),
                    json!({ clave: seccion.clone() }),
                )
                .await;
            assert_eq!(respuesta.status, StatusCode::OK);
        }
    }
    let v3 = h
        .post(&format!("/api/v1/bridges/{bridge_id}/versions"), None)
        .await;
    assert_eq!(v3.status, StatusCode::CREATED, "{}", v3.body);
    let v3 = v3.json();
    assert_eq!(v3["version"], json!(3));
    assert_eq!(
        v3["definition"], definicion_v1,
        "v3 == contenido congelado de la v1"
    );

    // El pasado NO cambió: v1 idéntica y la ejecución sigue en la v1.
    let detalle_v1 = h
        .get(&format!("/api/v1/bridges/{bridge_id}/versions/1"))
        .await;
    assert_eq!(detalle_v1.status, StatusCode::OK, "{}", detalle_v1.body);
    assert_eq!(detalle_v1.json()["definition"], definicion_v1);
    let (ejecucion_version,): (Uuid,) =
        sqlx::query_as("SELECT bridge_version_id FROM executions WHERE id = $1")
            .bind(ejecucion)
            .fetch_one(&h.pool)
            .await
            .expect("ejecución tras el rollback");
    assert_eq!(ejecucion_version, bridge_version_v1);
}
