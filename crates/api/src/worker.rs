//! Worker de la cola de ejecuciones (MVP2).
//!
//! La tabla `executions` **es** la cola: filas `pending` con `run_at <= now()`
//! reclamables con `FOR UPDATE SKIP LOCKED`. En MVP1 el único productor era el
//! plano síncrono, que reclamaba y corría inline; con el disparo asíncrono
//! (`Prefer: respond-async`), los triggers cron y el disparo manual hacen
//! falta un consumidor: este worker corre dentro del binario del api y
//! procesa la cola con el mismo motor que el plano síncrono
//! ([`crate::invoke::ejecutar_encolada`]).
//!
//! Detalles de diseño (design.md D5):
//! - instancia única en MVP2: con réplicas habría que elegir líder; la cola
//!   SKIP LOCKED protege al worker, no al scheduler;
//! - sondeo cada [`POLL_INTERVAL`] cuando la cola está vacía;
//! - un error de una ejecución se persiste como `failed` y el worker sigue;
//!   un error de infraestructura (BD) se registra y reintenta en el próximo
//!   tick.

// Módulo con `Result<_, Problem>` como todo el crate (mismo lint autorizado).
#![allow(clippy::result_large_err)]

use domain::definition::BridgeDefinition;
use engine::store::ExecutionStore;

use crate::invoke::RunOutcome;
use crate::problem::{self, Problem};
use crate::state::AppState;

/// Pausa del worker cuando la cola queda vacía (o falla la infraestructura).
pub const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

/// Arranca el worker en segundo plano para la vida del proceso.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            match procesar_pendiente(&state).await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(POLL_INTERVAL).await,
                Err(error) => {
                    tracing::warn!(?error, "worker: tick con error de infraestructura");
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            }
        }
    });
}

/// Reclama y corre UNA ejecución pendiente. Devuelve `Ok(false)` si la cola
/// está vacía. Público para los tests: montan la app sin worker y avanzan la
/// cola paso a paso de forma determinista.
pub async fn procesar_pendiente(state: &AppState) -> Result<bool, Problem> {
    let store = ExecutionStore::new(state.pool.clone());
    let Some(row) = store
        .claim_next()
        .await
        .map_err(|error| problem::internal(format!("worker: reclamar la siguiente: {error}")))?
    else {
        return Ok(false);
    };

    // Snapshot de la versión con la que la fila fue encolada (semántica de
    // congelado: lo que corre es lo que se publicó).
    let raw: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT definition FROM bridge_versions WHERE id = $1")
            .bind(row.bridge_version_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|error| problem::internal(format!("worker: cargar el snapshot: {error}")))?;
    let Some(raw) = raw else {
        // Sin snapshot no hay nada que correr: la cerramos `failed` para que
        // no quede `running` eternamente.
        let payload = domain::execution::ErrorPayload::new(
            domain::execution::ErrorCode::BlockFailed,
            format!(
                "la versión {} de la ejecución no existe (snapshot perdido)",
                row.bridge_version_id
            ),
        );
        persistir_fallo(state, row.id, &payload).await?;
        return Ok(true);
    };
    let definition: BridgeDefinition = serde_json::from_value(raw).map_err(|error| {
        problem::internal(format!("worker: snapshot no deserializable: {error}"))
    })?;

    // La validación del inputSchema solo aplica al disparo http (el body del
    // llamador). Cron y manual no traen entrada de nadie: corren tal cual.
    let validar_input = row.trigger_type == "http";

    match crate::invoke::ejecutar_encolada(
        state,
        row.id,
        row.workspace_id,
        definition,
        row.trigger_data.clone(),
        validar_input,
    )
    .await?
    {
        RunOutcome::Finished(outcome) => {
            tracing::info!(
                execution = %row.id,
                status = %outcome.status,
                "worker: ejecución terminada"
            );
        }
        RunOutcome::InputInvalid(error) => {
            tracing::warn!(execution = %row.id, %error, "worker: entrada inválida");
        }
    }
    Ok(true)
}

/// Cierra una ejecución `running` como `failed` con el error dado (usada
/// cuando el worker no puede correr la cadena: snapshot perdido).
async fn persistir_fallo(
    state: &AppState,
    execution_id: uuid::Uuid,
    error: &domain::execution::ErrorPayload,
) -> Result<(), Problem> {
    let payload = serde_json::to_value(error)
        .map_err(|e| problem::internal(format!("serializar el error: {e}")))?;
    sqlx::query(
        "UPDATE executions SET status = 'failed', error = $2, finished_at = now() \
         WHERE id = $1 AND status = 'running'",
    )
    .bind(execution_id)
    .bind(payload)
    .execute(&state.pool)
    .await
    .map_err(|e| problem::internal(format!("worker: cerrar la ejecución fallida: {e}")))?;
    Ok(())
}
