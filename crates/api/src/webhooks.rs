//! Entrega del resultado por webhook firmado (MVP2, spec `webhook-delivery`).
//!
//! Cuando una ejecución de un puente con webhook termina — con éxito o con
//! fallo — `invoke::programar_webhook` deja una fila `pending` en
//! `webhook_deliveries`. Este job la consume con el mismo patrón que la cola
//! de ejecuciones (design.md D4): `FOR UPDATE SKIP LOCKED`, firma HMAC-SHA256
//! del body crudo con el secreto del puente y reintentos persistentes con
//! backoff fijo 30 s → 2 m → 10 m → 1 h → 6 h (5 intentos); éxito solo 2xx.
//!
//! Headers de cada intento:
//! - `X-Integraciones-Signature: sha256=<hex>` — HMAC-SHA256 del body crudo
//!   con el secreto del puente (recuperable por el plano admin).
//! - `X-Integraciones-Timestamp` — unix seconds (frescura).
//! - `X-Integraciones-Delivery` — id ESTABLE de la entrega: reintentos de un
//!   mismo resultado comparten header y executionId (deduplicable).
//!
//! MVP2: backoff fijo no configurable; instancia única (design.md D5).

// Módulo con `Result<_, Problem>` como todo el crate (mismo lint autorizado).
#![allow(clippy::result_large_err)]

use std::time::Duration;

use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use uuid::Uuid;

use crate::problem::{self, Problem};
use crate::state::AppState;

/// Backoff fijo entre intentos, en segundos: 30 s, 2 m, 10 m, 1 h, 6 h.
/// La posición `attempts - 1` tras el intento fallido `attempts`.
pub const BACKOFF_SECS: [i64; 5] = [30, 120, 600, 3_600, 21_600];

/// Máximo de intentos por entrega; agotados → `failed` (sin más reintentos).
pub const MAX_ATTEMPTS: i32 = 5;

/// Timeout de la petición de entrega: un receptor lento no bloquea el job.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Arranca el job de entregas en segundo plano para la vida del proceso.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            match procesar_una_entrega(&state).await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(Duration::from_millis(500)).await,
                Err(error) => {
                    tracing::warn!(?error, "webhooks: tick con error de infraestructura");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    });
}

/// Reclama y entrega UNA entrega pendiente. `Ok(false)` = no hay trabajo.
/// Público para los tests: avanzan la cola entrega a entrega, manipulando
/// `next_retry_at` entre llamadas para simular el paso del tiempo.
pub async fn procesar_una_entrega(state: &AppState) -> Result<bool, Problem> {
    let pool = &state.pool;

    // 1. Claim: SKIP LOCKED, solo pendientes vencidas con intentos libres.
    //    El intento se cuenta y `next_retry_at` se empuja al backoff DENTRO de
    //    la transacción de claim: si el proceso cae a mitad del HTTP, la fila
    //    ya no es inmediatamente reclamable (y el intento cuenta).
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| problem::internal(format!("webhooks: abrir el claim: {error}")))?;
    let Some((delivery_id, execution_id, url)): Option<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, execution_id, url FROM webhook_deliveries \
         WHERE status = 'pending' AND attempts < $1 AND next_retry_at <= now() \
         ORDER BY next_retry_at, created_at \
         FOR UPDATE SKIP LOCKED \
         LIMIT 1",
    )
    .bind(MAX_ATTEMPTS)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| problem::internal(format!("webhooks: reclamar la entrega: {error}")))?
    else {
        return Ok(false);
    };
    let attempts: i32 =
        sqlx::query_scalar("UPDATE webhook_deliveries SET attempts = attempts + 1 WHERE id = $1 RETURNING attempts")
            .bind(delivery_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| problem::internal(format!("webhooks: contar el intento: {error}")))?;
    let backoff = BACKOFF_SECS
        .get((attempts - 1).max(0) as usize)
        .copied()
        .unwrap_or(*BACKOFF_SECS.last().expect("backoff no vacío"));
    sqlx::query(
        "UPDATE webhook_deliveries \
         SET next_retry_at = now() + make_interval(secs => $2::int), \
             last_attempt_at = now() \
         WHERE id = $1",
    )
    .bind(delivery_id)
    .bind(backoff as i32)
    .execute(&mut *tx)
    .await
    .map_err(|error| problem::internal(format!("webhooks: programar el reintento: {error}")))?;
    tx.commit()
        .await
        .map_err(|error| problem::internal(format!("webhooks: confirmar el claim: {error}")))?;

    // 2. Payload + secreto.
    let (secreto, payload) =
        construir_entrega(state, delivery_id, execution_id).await?;

    // 3. Envío firmado.
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| problem::internal(format!("webhooks: cliente http: {error}")))?;
    let timestamp = chrono::Utc::now().timestamp();
    let envio = client
        .post(&url)
        .header("X-Integraciones-Signature", firmar(secreto.as_bytes(), &payload))
        .header("X-Integraciones-Timestamp", timestamp.to_string())
        .header("X-Integraciones-Delivery", delivery_id.to_string())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(payload)
        .send()
        .await;

    // 4. Cierre del intento: 2xx delivered; otro resultado queda `pending`
    //    con el backoff ya programado; agotados los intentos → `failed`.
    let respuesta = match &envio {
        Ok(respuesta) => Some(respuesta.status().as_u16().to_string()),
        Err(error) => {
            tracing::warn!(delivery = %delivery_id, %error, "webhooks: envío fallido");
            None
        }
    };
    let exito = respuesta
        .as_deref()
        .map(|status| status.starts_with('2'))
        .unwrap_or(false);
    let estado_final = if exito {
        "delivered"
    } else if attempts >= MAX_ATTEMPTS {
        "failed"
    } else {
        "pending"
    };
    // `error` es solo para fallos de transporte/timeout: un código HTTP del
    // receptor ya queda en `response_status`.
    let detalle_error = if envio.is_err() {
        Some("error de transporte o timeout".to_string())
    } else {
        None
    };
    let status_code: Option<i32> = respuesta
        .as_deref()
        .and_then(|status| status.parse().ok());
    sqlx::query(
        "UPDATE webhook_deliveries \
         SET status = $2, response_status = $3, error = $4, \
             last_attempt_at = now(), \
             next_retry_at = CASE WHEN $2 = 'delivered' THEN NULL ELSE next_retry_at END \
         WHERE id = $1",
    )
    .bind(delivery_id)
    .bind(estado_final)
    .bind(status_code)
    .bind(detalle_error)
    .execute(pool)
    .await
    .map_err(|error| problem::internal(format!("webhooks: cerrar el intento: {error}")))?;

    tracing::info!(
        delivery = %delivery_id,
        execution = %execution_id,
        attempts,
        estado_final,
        "webhooks: intento cerrado"
    );
    Ok(true)
}

/// Construye el par (secreto en claro, body crudo) de una entrega: el sobre
/// final + metadatos (`bridgeSlug`, `status`, `finishedAt`), leído del estado
/// persistido — nunca de memoria del proceso.
async fn construir_entrega(
    state: &AppState,
    delivery_id: Uuid,
    execution_id: Uuid,
) -> Result<(String, String), Problem> {
    let secreto_cifrado: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT b.webhook_secret FROM webhook_deliveries d \
         JOIN executions e ON e.id = d.execution_id \
         JOIN bridges b ON b.id = e.bridge_id \
         WHERE d.id = $1",
    )
    .bind(delivery_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("webhooks: cargar el secreto: {error}")))?
    .flatten();
    let Some(secreto_cifrado) = secreto_cifrado else {
        return Err(problem::internal(
            "webhooks: la entrega no tiene secreto de puente (¿publicada antes del webhook?)",
        ));
    };
    let secreto = crate::vault::decrypt(&secreto_cifrado)
        .map_err(|error| problem::internal(format!("webhooks: descifrar el secreto: {error}")))?;

    type FilaEjecucion = (String, Option<Value>, Option<chrono::DateTime<chrono::Utc>>, String);
    let Some((status, error, finished_at, bridge_slug)): Option<FilaEjecucion> = sqlx::query_as(
        "SELECT e.status, e.error, e.finished_at, b.slug \
         FROM executions e JOIN bridges b ON b.id = e.bridge_id \
         WHERE e.id = $1",
    )
    .bind(execution_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("webhooks: cargar la ejecución: {error}")))?
    else {
        return Err(problem::internal(
            "webhooks: la entrega apunta a una ejecución inexistente",
        ));
    };

    let data: Option<Value> = sqlx::query_scalar(
        "SELECT output FROM execution_steps \
         WHERE execution_id = $1 AND status = 'succeeded' \
         ORDER BY seq DESC LIMIT 1",
    )
    .bind(execution_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("webhooks: cargar la salida: {error}")))?
    .flatten();

    // Sobre final idéntico al síncrono, más los metadatos de entrega.
    let mut payload = match status.as_str() {
        "succeeded" => json!({
            "success": true,
            "data": data.unwrap_or(Value::Null),
            "executionId": execution_id,
        }),
        _ => {
            let error = error
                .filter(|e| e.as_object().is_some_and(|e| !e.is_empty()))
                .unwrap_or_else(|| {
                    json!({ "code": "BLOCK_FAILED", "message": "la ejecución falló sin error estructurado" })
                });
            json!({
                "success": false,
                "error": error,
                "executionId": execution_id,
            })
        }
    };
    if let Some(map) = payload.as_object_mut() {
        map.insert("bridgeSlug".to_string(), json!(bridge_slug));
        map.insert("status".to_string(), json!(status));
        map.insert(
            "finishedAt".to_string(),
            finished_at
                .map(|at| json!(at.to_rfc3339()))
                .unwrap_or(Value::Null),
        );
    }

    Ok((secreto, payload.to_string()))
}

/// Firma HMAC-SHA256 del body crudo con el secreto del puente:
/// `sha256=<hex>`.
pub fn firmar(secreto: &[u8], body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secreto).expect("HMAC acepta claves de 32 bytes");
    mac.update(body.as_bytes());
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}
