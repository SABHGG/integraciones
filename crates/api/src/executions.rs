//! API REST de ejecuciones: listado con filtros + cursor, detalle con la
//! entrada del trigger y trace con el IO completo por bloque.
//!
//! Todo scoped al workspace de la api key (una ejecución de otro workspace es
//! 404, igual que bloques y puentes). Las filas las crea el plano de
//! invocación (tarea 4.4) y las mueve de estado el ejecutor (tareas 3.x):
//! este plano es de lectura.

// Los handlers devuelven `Problem` por valor como todo el crate; el lint de
// tamaño se autoriza a nivel de módulo igual que en blocks.rs.
#![allow(clippy::result_large_err)]

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use uuid::Uuid;

use crate::auth::AuthedKey;
use crate::cursor;
use crate::problem::{self, Problem};
use crate::rest::{self, Page};
use crate::state::AppState;

/// Estados válidos del filtro `status` (los mismos del CHECK de la tabla).
const STATUSES: [&str; 4] = ["pending", "running", "succeeded", "failed"];

/// Rutas de ejecuciones bajo `/api/v1`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/executions", get(list_executions))
        .route("/executions/{id}", get(get_execution))
        .route("/executions/{id}/trace", get(get_trace))
        .route("/executions/{id}/replay", post(replay))
}

// ---------------------------------------------------------------- DTOs ---

/// Fila de listado (liviana: sin `trigger_data`, que puede ser el body entero
/// del disparo).
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct ExecutionDto {
    id: Uuid,
    bridge_id: Uuid,
    bridge_version_id: Uuid,
    trigger_type: String,
    status: String,
    error: Option<Value>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

/// Resumen de un paso para el listado: estado y duración, sin IO (nada que
/// enmascarar). Sostiene el rastro en miniatura de la consola (MVP6).
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct PasoResumenDto {
    status: String,
    duration_ms: Option<i32>,
}

/// Ítem del listado: la ejecución liviana más el resumen aditivo de sus pasos.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionListItemDto {
    #[serde(flatten)]
    base: ExecutionDto,
    pasos: Vec<PasoResumenDto>,
}

/// Detalle completo: estado, fechas, entrada del trigger y la versión exacta
/// de la definición con la que corrió (spec `execution-store`).
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct ExecutionDetailDto {
    id: Uuid,
    bridge_id: Uuid,
    bridge_version_id: Uuid,
    trigger_type: String,
    trigger_data: Value,
    status: String,
    idempotency_key: Option<String>,
    error: Option<Value>,
    /// MVP3: credenciales usadas (pares credential/version, sin valores).
    credentials_used: Option<Value>,
    /// MVP3: ejecuciones derivadas por replay (vacío si no las hay).
    #[sqlx(skip)]
    replays: Vec<ReplayRefDto>,
    /// MVP4: quién disparó ({type: user|api_key|system, …}); NULL en /x.
    trigger_principal: Option<Value>,
    run_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Un paso del trace: entrada resuelta, salida, error y duración del bloque.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct StepDto {
    seq: i32,
    block_instance_id: String,
    block_name: String,
    status: String,
    input: Value,
    output: Option<Value>,
    error: Option<Value>,
    duration_ms: Option<i32>,
    created_at: DateTime<Utc>,
}

/// Query string del listado. Los campos libres (ids/fechas) llegan como texto
/// para responder 400 Problem Details ante valores inválidos.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionQuery {
    /// Filtra por puente.
    bridge_id: Option<String>,
    /// Filtra por estado (`pending|running|succeeded|failed`).
    status: Option<String>,
    /// Fecha de alta mínima (RFC 3339, inclusiva).
    from: Option<String>,
    /// Fecha de alta máxima (RFC 3339, inclusiva).
    to: Option<String>,
    /// Tamaño de página pedido (`1..=MAX_LIMIT`).
    limit: Option<String>,
    /// Cursor opaco de la página siguiente.
    cursor: Option<String>,
}

fn not_found() -> Problem {
    problem::not_found("la ejecución no existe en este workspace")
}

// ------------------------------------------------------------ Handlers ---

/// `GET /api/v1/executions`: listado paginado por cursor (created_at, id),
/// más nuevo primero, con filtros opcionales por puente, estado y rango de
/// fechas de alta.
async fn list_executions(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Query(query): Query<ExecutionQuery>,
) -> Result<Json<Page<ExecutionListItemDto>>, Problem> {
    let workspace_id = key.workspace()?;
    let limit = rest::page_limit(query.limit.as_deref())?;
    let after = match query.cursor.as_deref() {
        Some(raw) => Some(
            cursor::decode_execution_cursor(raw)
                .map_err(|_| problem::bad_request("el cursor es inválido"))?,
        ),
        None => None,
    };
    let bridge_id = match &query.bridge_id {
        Some(raw) => Some(
            Uuid::parse_str(raw)
                .map_err(|_| problem::bad_request(format!("bridgeId \"{raw}\" no es un UUID")))?,
        ),
        None => None,
    };
    let status = match &query.status {
        Some(status) if STATUSES.contains(&status.as_str()) => Some(status.as_str()),
        Some(status) => {
            return Err(problem::bad_request(format!(
                "status \"{status}\" no es válido (usa pending, running, succeeded o failed)"
            )));
        }
        None => None,
    };
    let parse_date = |raw: &str, field: &str| -> Result<DateTime<Utc>, Problem> {
        DateTime::parse_from_rfc3339(raw)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| {
                problem::bad_request(format!("{field} \"{raw}\" no es una fecha RFC 3339"))
            })
    };
    let from = query
        .from
        .as_deref()
        .map(|raw| parse_date(raw, "from"))
        .transpose()?;
    let to = query
        .to
        .as_deref()
        .map(|raw| parse_date(raw, "to"))
        .transpose()?;
    let (cursor_created_at, cursor_id) =
        after.map_or((None, None), |(ts, id)| (Some(ts), Some(id)));

    // Filtros opcionales con guardas `IS NULL`: una sola consulta para las
    // combinaciones, con binds tipados.
    let rows: Vec<ExecutionDto> = sqlx::query_as(
        "SELECT e.id, e.bridge_id, e.bridge_version_id, e.trigger_type, e.status, e.error, \
                e.started_at, e.finished_at, e.created_at \
         FROM executions e \
         WHERE e.workspace_id = $1 \
           AND ($2::uuid IS NULL OR e.bridge_id = $2) \
           AND ($3::text IS NULL OR e.status = $3) \
           AND ($4::timestamptz IS NULL OR e.created_at >= $4) \
           AND ($5::timestamptz IS NULL OR e.created_at <= $5) \
           AND ($6::timestamptz IS NULL OR (e.created_at, e.id) < ($6, $7)) \
         ORDER BY e.created_at DESC, e.id DESC \
         LIMIT $8",
    )
    .bind(workspace_id)
    .bind(bridge_id)
    .bind(status)
    .bind(from)
    .bind(to)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await
    .map_err(|error| rest::db_error("listar las ejecuciones", error))?;

    let mut rows = rows;
    let next_cursor = rest::next_cursor(&mut rows, limit, |execution| {
        cursor::encode_execution_cursor(execution.created_at, execution.id)
    });

    // Resumen de pasos de la página (MVP6, aditivo): una consulta acotada a
    // los ids de la página, con estado y duración por paso y SIN IO — nada
    // que enmascarar. Vacío en ejecuciones sin pasos aún (pending).
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let mut pasos: HashMap<Uuid, Vec<PasoResumenDto>> = HashMap::new();
    if !ids.is_empty() {
        let filas: Vec<(Uuid, String, Option<i32>)> = sqlx::query_as(
            "SELECT execution_id, status, duration_ms FROM execution_steps \
             WHERE execution_id = ANY($1) ORDER BY execution_id, seq",
        )
        .bind(&ids)
        .fetch_all(&state.pool)
        .await
        .map_err(|error| rest::db_error("listar los pasos de la página", error))?;
        for (execution_id, status, duration_ms) in filas {
            pasos
                .entry(execution_id)
                .or_default()
                .push(PasoResumenDto { status, duration_ms });
        }
    }

    Ok(Json(Page {
        items: rows
            .into_iter()
            .map(|base| {
                let pasos = pasos.remove(&base.id).unwrap_or_default();
                ExecutionListItemDto { base, pasos }
            })
            .collect(),
        next_cursor,
    }))
}

/// `GET /api/v1/executions/{id}`: detalle completo de la ejecución.
async fn get_execution(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
) -> Result<Json<ExecutionDetailDto>, Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "la ejecución no existe en este workspace")?;
    let row = sqlx::query_as::<_, ExecutionDetailDto>(
        "SELECT id, bridge_id, bridge_version_id, trigger_type, trigger_data, status, \
                idempotency_key, error, credentials_used, trigger_principal, run_at, \
                started_at, finished_at, created_at, updated_at \
         FROM executions WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| rest::db_error("cargar la ejecución", error))?
    .ok_or_else(not_found)?;
    // Enlace inverso (MVP3): las ejecuciones derivadas por replay, con su
    // estado y fecha; vacío para la mayoría.
    let mut row = row;
    row.replays = sqlx::query_as::<_, ReplayRefDto>(
        "SELECT id, status, created_at FROM executions \
         WHERE trigger_data->>'replayOf' = $1 ORDER BY created_at",
    )
    .bind(id.to_string())
    .fetch_all(&state.pool)
    .await
    .map_err(|error| rest::db_error("cargar los replays", error))?;
    Ok(Json(row))
}

/// Referencia de un replay para el detalle de la ejecución original.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct ReplayRefDto {
    id: Uuid,
    status: String,
    created_at: DateTime<Utc>,
}

/// `POST /api/v1/executions/{id}/replay` (MVP3, spec `execution-replay`):
/// crea una ejecución nueva con la MISMA versión congelada y la misma
/// entrada de trigger de la original (`trigger_data.replayOf` = id
/// original) y la encola `pending` para el worker normal — mismo timeout,
/// mismo masking, misma entrega webhook. El original no se muta.
async fn replay(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "la ejecución no existe en este workspace")?;
    let original: Option<(Uuid, Uuid, String, Value)> = sqlx::query_as(
        "SELECT bridge_id, bridge_version_id, trigger_type, trigger_data \
         FROM executions WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| rest::db_error("cargar la ejecución", error))?;

    let Some((bridge_id, version_id, trigger_type, mut trigger_data)) = original else {
        return Err(not_found());
    };
    if !trigger_data.is_object() {
        trigger_data = json!({});
    }
    if let Some(objeto) = trigger_data.as_object_mut() {
        objeto.insert("replayOf".to_string(), json!(id));
    }

    let nuevo_id = Uuid::now_v7();
    // Auditoría (D7): quién re-ejecuta.
    let disparador = key.trigger_principal();
    sqlx::query(
        "INSERT INTO executions (id, workspace_id, bridge_id, bridge_version_id, \
         trigger_type, trigger_data, trigger_principal, status, run_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', now())",
    )
    .bind(nuevo_id)
    .bind(workspace_id)
    .bind(bridge_id)
    .bind(version_id)
    .bind(&trigger_type)
    .bind(&trigger_data)
    .bind(&disparador)
    .execute(&state.pool)
    .await
    .map_err(|error| rest::db_error("encolar el replay", error))?;

    tracing::info!(original = %id, replay = %nuevo_id, "replay encolado");
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "executionId": nuevo_id,
            "replayOf": id,
            "status": "pending",
        })),
    ))
}

/// `GET /api/v1/executions/{id}/trace`: pasos en orden (`seq`) con IO
/// completo, duración por bloque y el punto exacto del fallo.
async fn get_trace(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
) -> Result<Json<Value>, Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "la ejecución no existe en este workspace")?;
    // El trace exige que la ejecución exista en el workspace (404 si no),
    // aunque todavía no tenga pasos.
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM executions WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|error| rest::db_error("cargar la ejecución", error))?;
    if exists == 0 {
        return Err(not_found());
    }
    let steps: Vec<StepDto> = sqlx::query_as(
        "SELECT seq, block_instance_id, block_name, status, input, output, error, \
                duration_ms, created_at \
         FROM execution_steps WHERE execution_id = $1 ORDER BY seq",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|error| rest::db_error("cargar el trace", error))?;
    Ok(Json(serde_json::json!({
        "executionId": id,
        "steps": steps,
    })))
}
