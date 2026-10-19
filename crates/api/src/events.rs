//! Publicación de eventos con fan-out a la cola (MVP3, spec `event-triggers`).
#![allow(clippy::result_large_err)]
//!
//! D1: sin log de eventos — `POST /api/v1/events/{topic}` resuelve los
//! suscriptores activos (última versión publicada con trigger `event` sobre
//! el topic) y encola una ejecución por suscriptor en la misma transacción.
//! Sin suscriptores el evento no se retiene (202 con cero ejecuciones).
//! La emisión al completar (`emit`) reutiliza el mismo mecanismo desde la
//! transacción de finalización de la ejecución.
//!
//! Límite documentado: sin orden ni entrega garantizada entre suscriptores;
//! cada suscriptor es una ejecución independiente con su propio trace.

use crate::auth::AuthedKey;
use crate::problem::{self, Problem};
use crate::state::AppState;
use axum::Extension;
use axum::Json;
use axum::extract::{Path, State};
use axum::routing::post;
use serde_json::{Value, json};
use uuid::Uuid;

/// `POST /api/v1/events/{topic}`: publica un evento con el body como payload
/// y encola una ejecución por cada puente suscrito activo. Sin suscriptores
/// responde `202` igualmente: el evento no se retiene.
async fn publicar(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(topic): Path<String>,
    payload: Option<Json<Value>>,
) -> Result<(axum::http::StatusCode, Json<Value>), Problem> {
    let workspace_id = key.workspace()?;
    if !domain::definition::validar_topic(&topic) {
        return Err(problem::bad_request(format!(
            "\"{topic}\" no es un topic válido (minúsculas, dígitos, puntos y guiones)"
        )));
    }
    let payload = payload.map(|Json(valor)| valor).unwrap_or(Value::Null);

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| problem::internal(format!("abrir la transacción: {error}")))?;
    let (event_id, ejecuciones) = encolar_fanout(&mut tx, workspace_id, &topic, &payload)
        .await
        .map_err(|error| problem::internal(format!("fan-out del evento: {error}")))?;
    tx.commit()
        .await
        .map_err(|error| problem::internal(format!("publicar el evento: {error}")))?;

    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(json!({
            "topic": topic,
            "eventId": event_id,
            "executions": ejecuciones,
        })),
    ))
}

/// Router del plano de administración.
pub fn router() -> axum::Router<AppState> {
    axum::Router::new().route("/events/{topic}", post(publicar))
}

/// Resuelve los suscriptores activos del topic en el workspace: última
/// versión publicada de cada puente cuyo trigger congelado es `event` sobre
/// ese topic. Varios puentes pueden suscribirse al mismo topic.
async fn suscriptores(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace_id: Uuid,
    topic: &str,
) -> Result<Vec<(Uuid, Uuid)>, sqlx::Error> {
    let filas: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT b.id, v.id \
         FROM bridges b \
         JOIN bridge_versions v ON v.bridge_id = b.id AND v.version = b.latest_version \
         WHERE b.workspace_id = $1 \
           AND v.definition->'trigger'->>'type' = 'event' \
           AND v.definition->'trigger'->>'topic' = $2 \
         ORDER BY b.slug",
    )
    .bind(workspace_id)
    .bind(topic)
    .fetch_all(&mut **tx)
    .await?;
    Ok(filas)
}

/// Fan-out transaccional: un `eventId` por publicación y una ejecución
/// `pending` por suscriptor con `trigger_data = {topic, eventId, payload,
/// receivedAt}` (la forma de `$.trigger` del suscriptor). Devuelve
/// `(eventId, ejecuciones encoladas)`; la escritura va toda por `tx` para
/// que el fan-out sea atómico con quien lo llama (la API o `emit`).
pub(crate) async fn encolar_fanout(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace_id: Uuid,
    topic: &str,
    payload: &Value,
) -> Result<(Uuid, usize), sqlx::Error> {
    let event_id = Uuid::now_v7();
    let suscriptores = suscriptores(tx, workspace_id, topic).await?;
    let trigger_data = json!({
        "topic": topic,
        "eventId": event_id,
        "payload": payload,
        "receivedAt": chrono::Utc::now().to_rfc3339(),
    });
    for (bridge_id, version_id) in &suscriptores {
        sqlx::query(
            "INSERT INTO executions (id, workspace_id, bridge_id, bridge_version_id, \
             trigger_type, trigger_data, status, run_at) \
             VALUES ($1, $2, $3, $4, 'event', $5, 'pending', now())",
        )
        .bind(Uuid::now_v7())
        .bind(workspace_id)
        .bind(bridge_id)
        .bind(version_id)
        .bind(&trigger_data)
        .execute(&mut **tx)
        .await?;
    }
    Ok((event_id, suscriptores.len()))
}
