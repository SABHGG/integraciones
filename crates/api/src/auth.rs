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
///
/// `workspace_id` es opcional porque la FK de `api_keys` lo es: una key sin
/// workspace sigue siendo válida para rutas globales, pero los recursos de
/// biblioteca (bloques) exigen workspace ([`AuthedKey::workspace`]).
#[derive(Debug, Clone, Serialize)]
pub struct AuthedKey {
    /// Id de la fila en `api_keys`.
    pub id: Uuid,
    /// Nombre legible de la key.
    pub name: String,
    /// Workspace al que está vinculada la key, si lo tiene.
    pub workspace_id: Option<Uuid>,
}

impl AuthedKey {
    /// Workspace obligatorio para los handlers de recursos (bloques, puentes,
    /// credenciales): sin vínculo se responde 403 Problem Details.
    // `Problem` es grande (~150 bytes) pero solo se construye en la ruta de
    // error; mismo criterio que el middleware de este módulo.
    #[allow(clippy::result_large_err)]
    pub fn workspace(&self) -> Result<Uuid, problem::Problem> {
        self.workspace_id.ok_or_else(|| {
            problem::forbidden(
                "la api key no está vinculada a un workspace; \
                 vincula la key (api_keys.workspace_id) para usar los recursos",
            )
        })
    }
}

pub fn hash_key(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}


/// Middleware del plano `/api/v1`: exige Bearer válido y no revocado.
// axum idiomático: el middleware devuelve Response tanto en éxito como en error;
// Response supera los 128 bytes, así que silenciamos el lint en lugar de boxear.
#[allow(clippy::result_large_err)]
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
    let row = sqlx::query_as::<_, (Uuid, String, Option<Uuid>)>(
        "SELECT id, name, workspace_id FROM api_keys WHERE key_hash = $1 AND revoked_at IS NULL",
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

    req.extensions_mut().insert(AuthedKey {
        id: row.0,
        name: row.1,
        workspace_id: row.2,
    });
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn authed(workspace_id: Option<Uuid>) -> AuthedKey {
        AuthedKey {
            id: Uuid::now_v7(),
            name: "dev".to_string(),
            workspace_id,
        }
    }

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

    #[test]
    fn una_key_sin_workspace_no_puede_usar_los_recursos() {
        let key = authed(None);
        let problem = key.workspace().expect_err("sin workspace");
        assert_eq!(problem.status, StatusCode::FORBIDDEN);
        assert!(
            problem.detail.unwrap().contains("workspace"),
            "el detalle debe nombrar el workspace"
        );
    }

    #[test]
    fn una_key_vinculada_devuelve_su_workspace() {
        let workspace_id = Uuid::now_v7();
        assert_eq!(authed(Some(workspace_id)).workspace().unwrap(), workspace_id);
    }
}
