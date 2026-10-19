//! Plano de invocación `/x/{slug}`: ejecución síncrona de puentes publicados,
//! separado de la administración por superficie (spec `bridge-invocation`).
//!
//! Contrato:
//!
//! - **Transporte** = HTTP semántico con Problem Details: 404 (slug inexistente
//!   o sin versión publicada), 405 (method no coincide con el trigger del
//!   puente), 413 (body por encima del límite), 415 (Content-Type distinto de
//!   `application/json`, v1), 400 (body con JSON malformado).
//! - **Resultado de ejecución** = siempre 200 con el sobre único
//!   `{success, data, error?, executionId}`; los códigos de error son estables
//!   (`BLOCK_FAILED`, `TIMEOUT`, `REF_UNRESOLVED`, `VALIDATION_FAILED`, …).
//! - **Idempotencia** = header `Idempotency-Key` opcional: si la key se repite
//!   se devuelve el resultado de la ejecución previa (unique parcial en la BD).
//!
//! Headers del llamador **jamás** entran al contexto ni al IO persistido
//! (design.md → "Contrato del trigger"): el trigger expone `{body, query}`.

// Los handlers devuelven `Problem` por valor como todo el crate; el lint de
// tamaño se autoriza a nivel de módulo igual que en blocks.rs.
#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::Response;
use axum::routing::{any, get};
use domain::definition::BridgeDefinition;
use domain::execution::ErrorPayload;
use engine::executor::{ChainRun, ExecutionOutcome, Executor};
use engine::invoker::SandboxInvoker;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::problem::{self, Problem};
use crate::rest;
use crate::state::AppState;

/// Límite del body del disparo HTTP (1 MB): por encima → 413 Problem Details.
pub const MAX_BODY_BYTES: usize = 1_000_000;

/// Header de idempotencia del disparo HTTP.
const IDEMPOTENCY_HEADER: &str = "idempotency-key";

/// Header `Prefer` (RFC 7240) y token que pide disparo asíncrono (MVP2).
const PREFER_HEADER: &str = "prefer";
const PREFER_RESPOND_ASYNC: &str = "respond-async";

/// Rutas del plano de invocación (se anidan bajo `/x` en `lib.rs`).
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{slug}", any(invoke_slug))
        .route("/{slug}/executions/{execution_id}", get(poll_result))
}

/// Puente publicado: la resolución del JOIN con su versión snapshot.
#[derive(Debug, sqlx::FromRow)]
struct PublishedBridge {
    bridge_id: Uuid,
    workspace_id: Uuid,
    bridge_version_id: Uuid,
    definition: Value,
}

// --------------------------------------------------------- Problemas ---

fn method_not_allowed(method: &Method, allowed: &str) -> Problem {
    Problem::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method-not-allowed",
        "Method Not Allowed",
    )
    .with_detail(format!(
        "el puente declara un disparador {allowed}; la petición usa {method}"
    ))
    .with_extension("allowed", json!(allowed))
}

fn payload_to_large() -> Problem {
    Problem::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        "payload-too-large",
        "Payload Too Large",
    )
    .with_detail(format!(
        "el body excede el límite de {MAX_BODY_BYTES} bytes"
    ))
}

fn unsupported_media_type(media_type: &str) -> Problem {
    Problem::new(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "unsupported-media-type",
        "Unsupported Media Type",
    )
    .with_detail(format!(
        "{media_type:?} no está soportado (v1 solo acepta application/json)"
    ))
}

// ---------------------------------------------------------- Handlers ---

/// `ANY /x/{slug}`: ejecuta el puente publicado con el slug dado.
async fn invoke_slug(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    request: Request,
) -> Result<Response, Problem> {
    let (parts, body) = request.into_parts();
    let method = parts.method;
    let raw_query = parts.uri.query().unwrap_or("");
    let idempotency_key = parts
        .headers
        .get(IDEMPOTENCY_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    // `Prefer: respond-async` (RFC 7240): el disparo encola y responde 202 en
    // cuanto la fila queda pendiente; el resultado se consulta por polling.
    // El resto de la lista de `Prefer` se ignora (solo el token importa).
    let respond_async = parts
        .headers
        .get(PREFER_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value.split(',').any(|part| {
                let token = part.trim().split(';').next().unwrap_or("").trim();
                token.eq_ignore_ascii_case(PREFER_RESPOND_ASYNC)
            })
        })
        .unwrap_or(false);

    // 1. Puente publicado por slug: el más reciente con versión publicada. El
    // plano no usa auth de administración; MVP1 no declara auth por puente.
    let published = sqlx::query_as::<_, PublishedBridge>(
        "SELECT b.id AS bridge_id, b.workspace_id, bv.id AS bridge_version_id, bv.definition \
         FROM bridges b \
         JOIN bridge_versions bv ON bv.bridge_id = b.id AND bv.version = b.latest_version \
         WHERE b.slug = $1 AND b.latest_version IS NOT NULL \
         ORDER BY b.created_at DESC \
         LIMIT 1",
    )
    .bind(&slug)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("cargar el puente: {error}")))?
    .ok_or_else(|| {
        problem::not_found(format!("no existe un puente publicado con slug {slug:?}"))
    })?;
    let definition: BridgeDefinition =
        serde_json::from_value(published.definition).map_err(|error| {
            problem::internal(format!(
                "la versión publicada no es un snapshot válido: {error}"
            ))
        })?;

    // 1.bis. Un puente con trigger cron no se expone en el plano de
    // invocación (spec `cron-triggers`): 404 indistinguible de un slug
    // inexistente, antes de cualquier otro chequeo.
    if definition.trigger.trigger_type == domain::definition::TriggerType::Cron {
        return Err(problem::not_found(format!(
            "no existe un puente publicado con slug {slug:?}"
        )));
    }

    // 2. Matching del method del trigger (insensible a mayúsculas).
    let allowed = definition.trigger.method.as_deref().unwrap_or("GET");
    if !allowed.eq_ignore_ascii_case(method.as_str()) {
        return Err(method_not_allowed(&method, allowed));
    }

    // 3. Idempotencia: la key repetida devuelve el resultado previo sin correr.
    // En async la repetida es un 202 con el executionId original y su Location
    // (spec `async-invocation`), sin crear una nueva ejecución.
    if let Some((original_id, envelope)) =
        replay_if_keyed(&state, published.workspace_id, idempotency_key.as_deref()).await?
    {
        return Ok(if respond_async {
            accepted_response(&slug, original_id)
        } else {
            envelope_response(envelope)
        });
    }

    // 4. Body según el method (GET/DELETE no lo consumen).
    let body_value = read_body(&method, &parts.headers, body).await?;

    // 5. Trigger normalizado por el tipo del disparo: HTTP = `{body, query}`.
    let query: BTreeMap<String, String> = serde_urlencoded::from_str(raw_query).unwrap_or_default();
    let trigger_data = json!({ "body": body_value, "query": query });

    // 6. La fila nace `pending`. En el camino síncrono el claim va en LA MISMA
    // transacción (un worker del binario jamás ve esta fila `pending`: la
    // corrida no puede duplicarse). En async el commit confirma solo el
    // encolado y responde 202: el worker la reclamará después.
    let execution_id = Uuid::now_v7();
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| problem::internal(format!("abrir la transacción: {error}")))?;
    let inserted = sqlx::query(
        "INSERT INTO executions (id, workspace_id, bridge_id, bridge_version_id, \
         trigger_type, trigger_data, status, idempotency_key, run_at) \
         VALUES ($1, $2, $3, $4, 'http', $5, 'pending', $6, now())",
    )
    .bind(execution_id)
    .bind(published.workspace_id)
    .bind(published.bridge_id)
    .bind(published.bridge_version_id)
    .bind(&trigger_data)
    .bind(&idempotency_key)
    .execute(&mut *tx)
    .await;
    if let Err(sqlx::Error::Database(db)) = &inserted
        && db.constraint() == Some("uq_executions_idempotency")
    {
        // Carrera de idempotencia (unique parcial): otra petición ganó el
        // INSERT; el drop del tx hace rollback de la fila perdedora.
        drop(tx);
        if let Some((original_id, envelope)) =
            replay_if_keyed(&state, published.workspace_id, idempotency_key.as_deref()).await?
        {
            return Ok(if respond_async {
                accepted_response(&slug, original_id)
            } else {
                envelope_response(envelope)
            });
        }
        return Err(problem::internal(
            "la carrera de idempotencia no dejó ejecución previa",
        ));
    }
    inserted.map_err(|error| problem::internal(format!("crear la ejecución: {error}")))?;

    if respond_async {
        tx.commit()
            .await
            .map_err(|error| problem::internal(format!("encolar la ejecución: {error}")))?;
        return Ok(accepted_response(&slug, execution_id));
    }

    // 7. Claim atómico de la corrida sync: `pending` → `running`.
    sqlx::query("UPDATE executions SET status = 'running', started_at = now() WHERE id = $1")
        .bind(execution_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| problem::internal(format!("marcar la ejecución running: {error}")))?;
    tx.commit()
        .await
        .map_err(|error| problem::internal(format!("crear la ejecución: {error}")))?;

    // 8-10. La corrida compartida (el worker de la cola usa el mismo camino).
    match ejecutar_encolada(
        &state,
        execution_id,
        published.workspace_id,
        definition,
        trigger_data,
        true,
    )
    .await?
    {
        RunOutcome::InputInvalid(error) => Ok(envelope_response(json!({
            "success": false,
            "error": error,
            "executionId": execution_id,
        }))),
        RunOutcome::Finished(outcome) => {
            // 11. Sobre único 200 con códigos de error estables.
            if outcome.is_success() {
                Ok(envelope_response(json!({
                    "success": true,
                    "data": outcome.output.unwrap_or(Value::Null),
                    "executionId": execution_id,
                })))
            } else {
                let error = outcome.error.unwrap_or_else(|| {
                    ErrorPayload::new(
                        domain::execution::ErrorCode::BlockFailed,
                        "la ejecución falló sin error estructurado",
                    )
                });
                Ok(envelope_response(json!({
                    "success": false,
                    "error": error,
                    "executionId": execution_id,
                })))
            }
        }
    }
}

/// `GET /x/{slug}/executions/{executionId}`: polling del resultado (MVP2).
///
/// Mientras la ejecución está `pending | running` responde `{success,
/// executionId, status}` sin `data`; al terminar responde el sobre final
/// idéntico al síncrono. El recurso queda acotado al slug: un executionId de
/// otro puente responde 404 sin filtrar nada.
async fn poll_result(
    State(state): State<AppState>,
    Path((slug, raw_execution_id)): Path<(String, String)>,
) -> Result<Response, Problem> {
    let execution_id = rest::parse_id(&raw_execution_id, "la ejecución no existe en este slug")?;
    let Some((status, error)): Option<(String, Option<Value>)> = sqlx::query_as(
        "SELECT e.status, e.error FROM executions e \
         JOIN bridges b ON b.id = e.bridge_id \
         WHERE e.id = $1 AND b.slug = $2",
    )
    .bind(execution_id)
    .bind(&slug)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("cargar la ejecución: {error}")))?
    else {
        return Err(problem::not_found(format!(
            "no existe la ejecución {execution_id} en el puente {slug:?}"
        )));
    };

    match status.as_str() {
        "pending" | "running" => Ok(json_response(json!({
            "success": true,
            "executionId": execution_id,
            "status": status,
        }))),
        "succeeded" => {
            let data: Option<Value> = sqlx::query_scalar(
                "SELECT output FROM execution_steps \
                 WHERE execution_id = $1 AND status = 'succeeded' \
                 ORDER BY seq DESC LIMIT 1",
            )
            .bind(execution_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|error| problem::internal(format!("cargar la salida: {error}")))?
            .flatten();
            Ok(json_response(json!({
                "success": true,
                "data": data.unwrap_or(Value::Null),
                "executionId": execution_id,
            })))
        }
        _ => {
            // failed: sobre de fallo idéntico al síncrono.
            let error = error
                .filter(|e| e.as_object().is_some_and(|e| !e.is_empty()))
                .unwrap_or_else(|| {
                    json!({
                        "code": "BLOCK_FAILED",
                        "message": "la ejecución falló sin error estructurado",
                    })
                });
            Ok(json_response(json!({
                "success": false,
                "error": error,
                "executionId": execution_id,
            })))
        }
    }
}

/// Resultado de correr una ejecución ya encolada (compartido por el plano
/// síncrono y el worker de la cola).
pub(crate) enum RunOutcome {
    /// La cadena corrió y el resultado está persistido.
    Finished(ExecutionOutcome),
    /// La entrada violó el inputSchema: la ejecución ya está cerrada `failed`
    /// sin pasos y el error está listo para el sobre.
    InputInvalid(ErrorPayload),
}

/// Corre una ejecución ya creada (y reclamada, o reclamable): validación de
/// entrada, bóveda, sandbox y persistencia del trace + cierre.
///
/// Es el camino compartido del disparo síncrono (que ya reclamó la fila en su
/// transacción) y del worker de la cola (que la reclamó con SKIP LOCKED).
pub(crate) async fn ejecutar_encolada(
    state: &AppState,
    execution_id: Uuid,
    workspace_id: Uuid,
    definition: BridgeDefinition,
    trigger_data: Value,
    validar_input: bool,
) -> Result<RunOutcome, Problem> {
    // La URL del webhook se extrae ANTES de mover la definición al closure
    // blocking (semántica de snapshot: la versión que corrió es la que entrega).
    let webhook_url = definition
        .webhook
        .as_ref()
        .map(|webhook| webhook.url.clone());
    // Validación de entrada (task 4.5): si el puente declara inputSchema, el
    // body debe cumplirlo ANTES de gastar isolate. Inválido → la ejecución
    // cierra `failed` con VALIDATION_FAILED, sin pasos y sin correr la cadena.
    // El worker solo la aplica al disparo http: cron/manual no traen entrada.
    if validar_input && let Some(schema) = &definition.input_schema {
        let body = trigger_data.get("body").cloned().unwrap_or(Value::Null);
        match domain::schema::validate(schema, &body) {
            Ok(()) => {}
            Err(domain::schema::SchemaError::Invalid { errors }) => {
                let detalles: Vec<Value> = errors
                    .iter()
                    .map(|error| json!({ "path": error.path, "message": error.message }))
                    .collect();
                let message = errors
                    .iter()
                    .map(|error| format!("{}: {}", error.path, error.message))
                    .collect::<Vec<_>>()
                    .join("; ");
                let error = ErrorPayload::new(
                    domain::execution::ErrorCode::ValidationFailed,
                    format!("la entrada del trigger no cumple el inputSchema: {message}"),
                )
                .with_details(json!(detalles));
                finish_failed(&state.pool, execution_id, &error)
                    .await
                    .map_err(|e| problem::internal(format!("persistir la validación: {e}")))?;
                return Ok(RunOutcome::InputInvalid(error));
            }
            // Un inputSchema con keywords no soportadas no debería haber
            // pasado la publicación; si llega aquí (snapshot manipulado),
            // falla cerrado sin correr nada.
            Err(domain::schema::SchemaError::Unsupported { path, reason }) => {
                return Err(problem::internal(format!(
                    "el inputSchema publicado no está soportado en {path}: {reason}"
                )));
            }
        }
    }

    // Bóveda del workspace: los secretos se precargan a memoria del host
    // ANTES del hilo blocking (el resolutor de ctx.credential es síncrono y no
    // puede tocar la BD desde el isolate). Nunca entran al contexto ni al IO.
    let boveda = cargar_boveda(&state.pool, workspace_id)
        .await
        .map_err(|error| problem::internal(format!("cargar la bóveda: {error}")))?;

    // MVP3: registro de credenciales usadas (auditoría de la bóveda
    // versionada). El resolutor es síncrono: anota cada resolución real en un
    // mapa compartido; se persiste con el trace, sin valores.
    let usadas: Arc<std::sync::Mutex<HashMap<String, (Uuid, Uuid)>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    let usadas_resolutor = Arc::clone(&usadas);

    // La corrida: el futuro del ejecutor no es Send (rquickjs full-async),
    // así que corre en un hilo blocking con runtime current-thread + LocalSet.
    let outcome = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime current-thread para el ejecutor");
        let local = tokio::task::LocalSet::new();
        local.block_on(&runtime, async {
            let invoker =
                SandboxInvoker::new().with_credential_resolver(Arc::new(move |name: String| {
                    match boveda.get(&name) {
                        Some((secreto, credential_id, version_id)) => {
                            usadas_resolutor
                                .lock()
                                .expect("registro de credenciales usadas")
                                .insert(name, (*credential_id, *version_id));
                            Some(secreto.clone())
                        }
                        None => None,
                    }
                }));
            let executor = Executor::new(invoker);
            executor.run(ChainRun::new(&definition, trigger_data)).await
        })
    })
    .await
    .expect("hilo blocking de la ejecución");

    // Persistencia: pasos con IO completo + cierre en UNA transacción (spec
    // `execution-store`: los pasos se persisten con la avanzación de estado).
    let credentials_used = credential_usadas_json(&usadas);
    persist_outcome(&state.pool, execution_id, &outcome, credentials_used)
        .await
        .map_err(|error| problem::internal(format!("persistir el resultado: {error}")))?;

    // Webhook de resultado (MVP2): si el puente lo declara, la URL congelada
    // se copia a la ejecución (semántica de snapshot: la versión que corrió es
    // la que entrega) y la entrega queda programada `pending` — con éxito o
    // con fallo; la firma y los reintentos viven en `webhooks`.
    if let Some(url) = webhook_url.as_deref() {
        programar_webhook(&state.pool, execution_id, url)
            .await
            .map_err(|error| problem::internal(format!("programar el webhook: {error}")))?;
    }

    Ok(RunOutcome::Finished(outcome))
}

/// Copia la URL del webhook a `executions.webhook_url` y crea la fila de
/// entrega `pending` (inmediatamente reclamable) en una transacción.
async fn programar_webhook(
    pool: &sqlx::PgPool,
    execution_id: Uuid,
    url: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE executions SET webhook_url = $2 WHERE id = $1")
        .bind(execution_id)
        .bind(url)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO webhook_deliveries (id, execution_id, url, status, next_retry_at) \
         VALUES ($1, $2, $3, 'pending', now())",
    )
    .bind(Uuid::now_v7())
    .bind(execution_id)
    .bind(url)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

// --------------------------------------------------------- Interior ---

/// 202 del disparo asíncrono: `{success, executionId}` + `Location` del
/// recurso de resultado (spec `async-invocation`).
fn accepted_response(slug: &str, execution_id: Uuid) -> Response {
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::LOCATION,
            format!("/x/{slug}/executions/{execution_id}"),
        )
        .body(Body::from(
            json!({ "success": true, "executionId": execution_id }).to_string(),
        ))
        .expect("202: builder con status válido no puede fallar")
}

/// Respuesta JSON 200 del plano de invocación.
fn json_response(envelope: Value) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(envelope.to_string()))
        .expect("sobre: builder con status válido no puede fallar")
}

/// Lee y valida el body del disparo: 413 si excede el límite, 415 Content-Type
/// equivocado, 400 JSON malformado. GET/DELETE: el body simplemente se ignora.
async fn read_body(method: &Method, headers: &HeaderMap, body: Body) -> Result<Value, Problem> {
    let consumes_body = matches!(method.as_str(), "POST" | "PUT" | "PATCH");
    if !consumes_body {
        return Ok(Value::Null);
    }
    let bytes = to_bytes(body, MAX_BODY_BYTES)
        .await
        .map_err(|_| payload_to_large())?;
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if essence != "application/json" {
        return Err(unsupported_media_type(content_type));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        Problem::new(StatusCode::BAD_REQUEST, "bad-request", "Bad Request")
            .with_detail(format!("el body no es JSON válido: {error}"))
    })
}

/// Replay solo si la petición trae key: se usa dos veces (pre-check y tras el
/// INSERT perdedor de la carrera). Devuelve el id de la ejecución original y
/// su sobre (para el Location del replay async).
async fn replay_if_keyed(
    state: &AppState,
    workspace_id: Uuid,
    key: Option<&str>,
) -> Result<Option<(Uuid, Value)>, Problem> {
    match key {
        Some(key) => replay_once(state, workspace_id, key).await,
        None => Ok(None),
    }
}

///
/// `data` = salida del último paso succeeded (`executions` no persiste la
/// salida como columna; los pasos sí llevan ese IO tarea 4.3).
async fn replay_once(
    state: &AppState,
    workspace_id: Uuid,
    key: &str,
) -> Result<Option<(Uuid, Value)>, Problem> {
    let Some((id, status, error)) = sqlx::query_as::<_, (Uuid, String, Option<Value>)>(
        "SELECT id, status, error FROM executions \
         WHERE workspace_id = $1 AND idempotency_key = $2 \
         ORDER BY created_at DESC \
         LIMIT 1",
    )
    .bind(workspace_id)
    .bind(key)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("idempotencia: {error}")))?
    else {
        return Ok(None);
    };
    let data: Option<Value> = sqlx::query_scalar(
        "SELECT output FROM execution_steps \
         WHERE execution_id = $1 AND status = 'succeeded' \
         ORDER BY seq DESC \
         LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("idempotencia: {error}")))?
    .flatten();
    let error = error.filter(|error| error.as_object().is_some_and(|e| !e.is_empty()));
    let success = status == "succeeded";
    Ok(Some((
        id,
        json!({
            "success": success,
            "data": data.unwrap_or(Value::Null),
            "error": error.unwrap_or(Value::Null),
            "executionId": id,
        }),
    )))
}

/// Fachada del sobre de invocación: 200, `application/json`.
fn envelope_response(envelope: Value) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(envelope.to_string()))
        .expect("sobre: builder con status válido no puede fallar")
}

/// Precarga la bóveda del workspace: nombre → (secreto, credential_id,
/// version_id) de la VERSIÓN VIGENTE de cada credencial (MVP3, bóveda
/// versionada). Pocos secretos por workspace, una consulta. Los bytes están
/// cifrados en reposo (AES-256-GCM, `vault`); se descifran aquí y el secreto
/// en claro solo vive en memoria del host.
type EntradaBoveda = (String, Uuid, Uuid);

async fn cargar_boveda(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
) -> Result<HashMap<String, EntradaBoveda>, sqlx::Error> {
    let filas: Vec<(String, Vec<u8>, Uuid, Uuid)> = sqlx::query_as(
        "SELECT c.name, cv.secret_encrypted, c.id, cv.id \
         FROM credentials c \
         JOIN credential_versions cv ON cv.id = c.current_version_id \
         WHERE c.workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    let mut boveda = HashMap::with_capacity(filas.len());
    for (name, bytes, credential_id, version_id) in filas {
        let secreto = crate::vault::decrypt(&bytes).map_err(|error| {
            sqlx::Error::Configuration(
                format!("descifrar la credencial \"{name}\": {error}").into(),
            )
        })?;
        boveda.insert(name, (secreto, credential_id, version_id));
    }
    Ok(boveda)
}

/// Serializa el registro de credenciales usadas (pares credential/version,
/// con el nombre para lectura humana; jamás valores).
fn credential_usadas_json(
    usadas: &std::sync::Mutex<HashMap<String, (Uuid, Uuid)>>,
) -> Option<Value> {
    let guard = usadas.lock().expect("registro de credenciales usadas");
    if guard.is_empty() {
        return None;
    }
    let entradas: Vec<Value> = guard
        .iter()
        .map(|(name, (credential_id, version_id))| {
            json!({
                "name": name,
                "credentialId": credential_id,
                "versionId": version_id,
            })
        })
        .collect();
    Some(json!({ "credentialsUsed": entradas }))
}

/// Cierra la ejecución como `failed` con error estructurado y sin pasos
/// (validación de entrada rechazada antes de invocar el sandbox).
async fn finish_failed(
    pool: &sqlx::PgPool,
    execution_id: Uuid,
    error: &ErrorPayload,
) -> Result<(), sqlx::Error> {
    let error =
        serde_json::to_value(error).map_err(|error| sqlx::Error::Configuration(error.into()))?;
    sqlx::query(
        "UPDATE executions SET status = 'failed', error = $2, finished_at = now() WHERE id = $1",
    )
    .bind(execution_id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Persiste la corrida: pasos con IO completo + cierre del estado en UNA
/// transacción de `executions`.
async fn persist_outcome(
    pool: &sqlx::PgPool,
    execution_id: Uuid,
    outcome: &ExecutionOutcome,
    credentials_used: Option<Value>,
) -> Result<(), sqlx::Error> {
    let status_final = outcome.status.as_str();
    let execution_error = outcome
        .error
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| sqlx::Error::Configuration(error.into()))?;
    let mut tx = pool.begin().await?;
    for step in &outcome.steps {
        let step_input = serde_json::to_value(&step.input)
            .map_err(|error| sqlx::Error::Configuration(error.into()))?;
        let step_output = step
            .output
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| sqlx::Error::Configuration(error.into()))?;
        let step_error = step
            .error
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| sqlx::Error::Configuration(error.into()))?;
        sqlx::query(
            "INSERT INTO execution_steps (execution_id, seq, block_instance_id, block_name, \
             status, input, output, error, duration_ms) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(execution_id)
        .bind(step.seq)
        .bind(&step.block_instance_id)
        .bind(&step.block_name)
        .bind(step.status.as_str())
        .bind(&step_input)
        .bind(step_output)
        .bind(step_error)
        .bind(step.duration_ms)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE executions \
         SET status = $2, error = $3, credentials_used = COALESCE($4, credentials_used), \
             finished_at = now() \
         WHERE id = $1",
    )
    .bind(execution_id)
    .bind(status_final)
    .bind(execution_error)
    .bind(credentials_used)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
