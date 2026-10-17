//! Auth del plano de administración: `Authorization: Bearer <api key>`.
//! El key crudo nunca se guarda: se compara SHA-256 contra `api_keys.key_hash`.

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::problem;
use crate::state::AppState;

/// Identidad de la API key autenticada, disponible para los handlers vía
/// `Extension<AuthedKey>`.
#[derive(Debug, Clone, Serialize)]
pub struct AuthedKey {
    pub id: Uuid,
    pub name: String,
}

pub fn hash_key(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_key_produce_sha256_hex_conocido() {
        // Vector verificado contra `hash-key "dev-key-123"` en el smoke test
        assert_eq!(
            hash_key("dev-key-123"),
            "0f11c9ecaecabe613512ae472855ae9cb7d9639bc8c3fe85e0357efbf4739cd4"
        );
    }

    #[test]
    fn hash_key_es_determinista_y_distinto_por_entrada() {
        assert_eq!(hash_key("abc"), hash_key("abc"));
        assert_ne!(hash_key("abc"), hash_key("abd"));
        assert_eq!(hash_key("").len(), 64); // SHA-256 = 64 hex
    }
}

/// Middleware del plano `/api/v1`: exige Bearer válido y no revocado.
pub async fn require_api_key(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let header = req.headers().get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    let token = header
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty());

    let token = match token {
        Some(t) => t,
        None => {
            return Err(problem::unauthorized(
                "Falta el header Authorization: Bearer <api key>",
            )
            .into_response());
        }
    };

    let hash = hash_key(token);
    let row = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, name FROM api_keys WHERE key_hash = $1 AND revoked_at IS NULL",
    )
    .bind(&hash)
    .fetch_optional(&state.pool)
    .await;

    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return Err(problem::unauthorized("API key inválida o revocada").into_response()),
        Err(e) => {
            tracing::error!(error = %e, "fallo al validar api key");
            return Err(problem::internal("fallo al validar la api key").into_response());
        }
    };

    // best-effort: actualizar último uso sin bloquear la petición si falla
    let _ = sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE id = $1")
        .bind(row.0)
        .execute(&state.pool)
        .await;

    req.extensions_mut().insert(AuthedKey { id: row.0, name: row.1 });
    Ok(next.run(req).await)
}
