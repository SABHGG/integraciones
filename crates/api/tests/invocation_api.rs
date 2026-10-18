//! Tests de integración del plano de invocación `/x/{slug}` contra un
//! Postgres **real**.
//!
//! Cubren la tarea 4.4: matching del method del trigger, errores de transporte
//! con Problem Details (404/405/413/415), ejecución síncrona con el sandbox
//! real y el sobre único 200 `{success, data, error?, executionId}` con
//! códigos de error estables, más la idempotencia opcional por header.
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
            "los fallos de transporte del plano /x son Problem Details: {}",
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
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'inv-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces previos");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'inv-test%'")
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
    let slug = format!("inv-test-{workspace_id}");
    let key_id = Uuid::now_v7();
    let raw_key = format!("inv-test-key-{workspace_id}");

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
        .bind(format!("inv-test-{raw_key}"))
        .bind(hash_key(&raw_key))
        .execute(pool)
        .await
        .expect("api key");

    (raw_key, key_id)
}

impl Harness {
    /// Petición al plano de invocación `/x/{slug}` (sin auth de administración).
    async fn x(
        &self,
        method: Method,
        slug: &str,
        body: Option<(&str, Value)>,
        idempotency_key: Option<&str>,
    ) -> Resp {
        let mut builder = Request::builder().method(method).uri(format!("/x/{slug}"));
        if let Some((content_type, _)) = &body {
            builder = builder.header(header::CONTENT_TYPE, content_type.to_string());
        }
        if let Some(key) = idempotency_key {
            builder = builder.header("Idempotency-Key", key);
        }
        let body = match body {
            Some((_, value)) => Body::from(value.to_string()),
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
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
            .await
            .expect("cuerpo legible");

        Resp {
            status,
            content_type,
            body: String::from_utf8(bytes.to_vec()).expect("cuerpo UTF-8"),
        }
    }

    /// Petición al plano de administración (con auth Bearer).
    async fn admin_get(&self, uri: &str) -> Resp {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {}", self.key))
                    .body(Body::empty())
                    .expect("petición válida"),
            )
            .await
            .expect("respuesta del router");

        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
            .await
            .expect("cuerpo legible");
        Resp {
            status,
            content_type,
            body: String::from_utf8(bytes.to_vec()).expect("cuerpo UTF-8"),
        }
    }

    /// Siembra un puente publicado (v1) con el trigger y la cadena dados: la
    /// definición snapshot ya lleva el JS de cada instancia embebido.
    async fn bridge(&self, slug: &str, method: &str, blocks: Value) -> Uuid {
        self.bridge_con_schema(slug, method, None, blocks).await
    }

    /// Igual que [`Self::bridge`] pero con `inputSchema` opcional (task 4.5).
    async fn bridge_con_schema(
        &self,
        slug: &str,
        method: &str,
        input_schema: Option<Value>,
        blocks: Value,
    ) -> Uuid {
        let bridge_id = Uuid::now_v7();
        let definition = json!({
            "trigger": { "type": "http", "method": method, "path": format!("/{slug}") },
            "inputSchema": input_schema,
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

        bridge_id
    }

    /// Número de ejecuciones del workspace de test.
    async fn execution_count(&self) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM executions WHERE workspace_id = $1")
            .bind(self.workspace_id)
            .fetch_one(&self.pool)
            .await
            .expect("conteo de ejecuciones")
    }
}

/// Bloque custom_logic que duplica `input.total`.
fn bloque_duplicar(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "code": "export default function(input, ctx) {\n    return { total: input.total * 2 };\n}",
        "input": { "total": "$.trigger.body.total" },
        "config": {},
        "outputSchema": { "type": "object" }
    })
}

#[tokio::test]
async fn invoca_un_puente_publicado_con_sobre_200_y_ejecucion_persistida() {
    let Some(h) = begin().await else { return };

    let js_envolver = "export default function(input, ctx) {\n    return { wrapped: input };\n}";
    h.bridge(
        "eco",
        "POST",
        json!([
            bloque_duplicar("b1", "duplicar"),
            {
                "id": "b2",
                "name": "envolver",
                "code": js_envolver,
                "input": { "entrada": "$.duplicar" },
                "config": {},
                "outputSchema": { "type": "object" }
            }
        ]),
    )
    .await;

    // Ejecución exitosa: 200 con sobre {success, data, executionId}.
    let respuesta = h
        .x(
            Method::POST,
            "eco",
            Some(("application/json", json!({ "total": 3 }))),
            None,
        )
        .await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], true, "{}", sobre);
    assert_eq!(
        sobre["data"],
        json!({ "wrapped": { "entrada": { "total": 6 } } }),
        "data = salida del último bloque"
    );
    assert!(sobre["error"].is_null());
    let execution_id = sobre["executionId"]
        .as_str()
        .expect("executionId")
        .to_string();

    // La ejecución queda persistida: visible por el plano de administración con
    // trigger HTTP normalizado ({body, query}) y trace completo en orden.
    let detalle = h
        .admin_get(&format!("/api/v1/executions/{execution_id}"))
        .await;
    assert_eq!(detalle.status, StatusCode::OK, "{}", detalle.body);
    let fila = detalle.json();
    assert_eq!(fila["status"], "succeeded");
    assert_eq!(fila["triggerType"], "http");
    assert_eq!(
        fila["triggerData"]["body"]["total"], 3,
        "entrada del trigger"
    );
    assert!(fila["bridgeVersionId"].as_str().is_some());

    let trace = h
        .admin_get(&format!("/api/v1/executions/{execution_id}/trace"))
        .await;
    let trace = trace.json();
    let steps = trace["steps"].as_array().expect("pasos");
    assert_eq!(steps.len(), 2, "{}", trace);
    assert_eq!(steps[0]["seq"], 1);
    assert_eq!(steps[0]["blockName"], "duplicar");
    assert_eq!(steps[0]["status"], "succeeded");
    assert_eq!(steps[0]["output"]["total"], 6);
    assert_eq!(steps[1]["blockName"], "envolver");
    assert_eq!(steps[1]["output"]["wrapped"]["entrada"]["total"], 6);

    // Method del trigger: mismatch → 405 Problem Details.
    h.x(Method::GET, "eco", None, None)
        .await
        .problem(StatusCode::METHOD_NOT_ALLOWED);

    // Slug inexistente → 404 Problem Details, sin sobre de ejecución.
    h.x(
        Method::POST,
        "no-existe",
        Some(("application/json", json!({}))),
        None,
    )
    .await
    .problem(StatusCode::NOT_FOUND);

    // Content-Type no soportado → 415 Problem Details.
    h.x(
        Method::POST,
        "eco",
        Some(("text/plain", json!({ "total": 1 }))),
        None,
    )
    .await
    .problem(StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Body por encima del límite → 413 Problem Details.
    let enorme = json!({ "blob": "x".repeat(1_100_000) });
    h.x(
        Method::POST,
        "eco",
        Some(("application/json", enorme)),
        None,
    )
    .await
    .problem(StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn body_malformado_responde_400_problem() {
    let Some(h) = begin().await else { return };

    let js_echo = "export default function(input, ctx) {\n    return input;\n}";
    h.bridge(
        "echo",
        "POST",
        json!([{ "id": "b1", "name": "echo", "code": js_echo, "input": {}, "config": {} }]),
    )
    .await;

    let response = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/x/echo")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{\"total\": ")) // JSON truncado
                .expect("petición válida"),
        )
        .await
        .expect("respuesta del router");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let resp = Resp {
        status,
        content_type,
        body: String::from_utf8(
            axum::body::to_bytes(response.into_body(), 1 << 20)
                .await
                .expect("cuerpo")
                .to_vec(),
        )
        .expect("UTF-8"),
    };
    resp.problem(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sobre_200_con_block_failed_y_error_estructurado() {
    let Some(h) = begin().await else { return };

    let js_boom = "export default function(input, ctx) {\n    throw new Error(\"boom\");\n}";
    h.bridge(
        "rompe",
        "POST",
        json!([{
            "id": "b1",
            "name": "explota",
            "code": js_boom,
            "input": {},
            "config": {}
        }]),
    )
    .await;

    // El fallo de ejecución NO es error de transporte: 200 con success=false.
    let respuesta = h
        .x(
            Method::POST,
            "rompe",
            Some(("application/json", json!({}))),
            None,
        )
        .await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], false, "{}", sobre);
    assert_eq!(sobre["error"]["code"], "BLOCK_FAILED", "código estable");
    assert_eq!(sobre["error"]["block"], "explota", "el bloque culpable");
    assert!(sobre["error"]["message"].as_str().is_some());
    assert!(sobre["data"].is_null());
    let execution_id = sobre["executionId"]
        .as_str()
        .expect("executionId")
        .to_string();

    // La ejecución queda como failed con error estructurado.
    let detalle = h
        .admin_get(&format!("/api/v1/executions/{execution_id}"))
        .await
        .json();
    assert_eq!(detalle["status"], "failed");
    assert_eq!(detalle["error"]["code"], "BLOCK_FAILED");
    assert!(detalle["finishedAt"].is_string());
}

#[tokio::test]
async fn get_acepta_query_params_y_el_trigger_los_expone() {
    let Some(h) = begin().await else { return };

    let js_echo = "export default function(input, ctx) {\n    return input;\n}";
    h.bridge(
        "consulta",
        "GET",
        json!([{
            "id": "b1",
            "name": "eco",
            "code": js_echo,
            "input": { "q": "$.trigger.query.q" },
            "config": {}
        }]),
    )
    .await;

    let respuesta = h.x(Method::GET, "consulta?q=hola", None, None).await;
    // GET con query: el sobre viaja en el cuerpo (el plano no define redirect).
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], true);
    assert_eq!(
        sobre["data"],
        json!({ "q": "hola" }),
        "query → $.trigger.query"
    );
}

#[tokio::test]
async fn idempotency_key_repetida_devuelve_la_ejecucion_previa() {
    let Some(h) = begin().await else { return };

    h.bridge("eco", "POST", json!([bloque_duplicar("b1", "duplicar")]))
        .await;

    // Primera invocación: ejecuta de verdad.
    let primera = h
        .x(
            Method::POST,
            "eco",
            Some(("application/json", json!({ "total": 5 }))),
            Some("reintento-1"),
        )
        .await;
    assert_eq!(primera.status, StatusCode::OK, "{}", primera.body);
    let sobre_1 = primera.json();
    assert_eq!(sobre_1["success"], true);
    let execution_1 = sobre_1["executionId"].as_str().expect("executionId");
    assert_eq!(h.execution_count().await, 1);

    // Reintento con la MISMA key: mismo resultado, sin nueva ejecución.
    let segunda = h
        .x(
            Method::POST,
            "eco",
            Some(("application/json", json!({ "total": 5 }))),
            Some("reintento-1"),
        )
        .await;
    assert_eq!(segunda.status, StatusCode::OK, "{}", segunda.body);
    let sobre_2 = segunda.json();
    assert_eq!(sobre_2["executionId"], execution_1, "misma ejecución");
    assert_eq!(sobre_2["data"], sobre_1["data"]);
    assert_eq!(h.execution_count().await, 1, "no se duplica la ejecución");

    // Otra key: sí crea una ejecución nueva.
    let tercera = h
        .x(
            Method::POST,
            "eco",
            Some(("application/json", json!({ "total": 7 }))),
            Some("reintento-2"),
        )
        .await;
    let sobre_3 = tercera.json();
    assert_ne!(sobre_3["executionId"], execution_1);
    assert_eq!(h.execution_count().await, 2);

    // Reintento de una ejecución fallida: el sobre reproduce el fallo.
    h.bridge(
        "rompe",
        "POST",
        json!([{
            "id": "b1",
            "name": "explota",
            "code": "export default function() {\n    throw new Error(\"boom\");\n}",
            "input": {},
            "config": {}
        }]),
    )
    .await;
    let fallida = h
        .x(
            Method::POST,
            "rompe",
            Some(("application/json", json!({}))),
            Some("reintento-fallo"),
        )
        .await;
    let sobre_f = fallida.json();
    assert_eq!(sobre_f["success"], false);
    assert_eq!(sobre_f["error"]["code"], "BLOCK_FAILED");
    let repite = h
        .x(
            Method::POST,
            "rompe",
            Some(("application/json", json!({}))),
            Some("reintento-fallo"),
        )
        .await;
    let sobre_r = repite.json();
    assert_eq!(sobre_r["executionId"], sobre_f["executionId"]);
    assert_eq!(sobre_r["error"]["code"], "BLOCK_FAILED");
    assert_eq!(h.execution_count().await, 3, "una ejecución nueva, no dos");
}

#[tokio::test]
async fn inputschema_invalido_da_validacion_failed_sin_gastar_isolate() {
    let Some(h) = begin().await else { return };

    let js_marca = "export default function(input, ctx) {\n    return { visto: true };\n}";
    h.bridge_con_schema(
        "validado",
        "POST",
        Some(json!({
            "type": "object",
            "required": ["total"],
            "properties": { "total": { "type": "integer" } }
        })),
        json!([{
            "id": "b1",
            "name": "marca",
            "code": js_marca,
            "input": {},
            "config": {}
        }]),
    )
    .await;

    // Body que viola el inputSchema → 200 con VALIDATION_FAILED (sobre, no
    // error de transporte) y SIN ejecutar la cadena.
    let respuesta = h
        .x(
            Method::POST,
            "validado",
            Some(("application/json", json!({ "otro": 1 }))),
            None,
        )
        .await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], false, "{}", sobre);
    assert_eq!(
        sobre["error"]["code"], "VALIDATION_FAILED",
        "código estable"
    );
    let mensaje = sobre["error"]["message"].as_str().expect("mensaje");
    assert!(mensaje.contains("total"), "mensaje quirúrgico: {mensaje}");
    assert!(sobre["data"].is_null());
    let execution_id = sobre["executionId"]
        .as_str()
        .expect("executionId")
        .to_string();

    // La ejecución queda failed con el error estructurado y SIN pasos: no se
    // gastó isolate (la cadena ni arrancó).
    let detalle = h
        .admin_get(&format!("/api/v1/executions/{execution_id}"))
        .await
        .json();
    assert_eq!(detalle["status"], "failed");
    assert_eq!(detalle["error"]["code"], "VALIDATION_FAILED");
    assert!(detalle["finishedAt"].is_string());
    let trace = h
        .admin_get(&format!("/api/v1/executions/{execution_id}/trace"))
        .await
        .json();
    assert_eq!(trace["steps"], json!([]), "sin pasos: {trace}");

    // Body con tipo equivocado → mismo contrato.
    let respuesta = h
        .x(
            Method::POST,
            "validado",
            Some(("application/json", json!({ "total": "no-soy-entero" }))),
            None,
        )
        .await;
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], false);
    assert_eq!(sobre["error"]["code"], "VALIDATION_FAILED");

    // Body conforme → ejecuta la cadena normal.
    let respuesta = h
        .x(
            Method::POST,
            "validado",
            Some(("application/json", json!({ "total": 2 }))),
            None,
        )
        .await;
    assert_eq!(respuesta.status, StatusCode::OK, "{}", respuesta.body);
    let sobre = respuesta.json();
    assert_eq!(sobre["success"], true, "{}", sobre);
    assert_eq!(sobre["data"], json!({ "visto": true }));
}
