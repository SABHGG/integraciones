//! Bóveda de credenciales del plano admin (task 4.7).
//!
//! El secreto es **write-only**: se acepta en `POST` (creación) y `PATCH`
//! (rotación), se cifra en reposo con `vault::encrypt` y jamás se devuelve en
//! ninguna lectura — solo metadatos (`id`, `name`, `description`, fechas).
//! Los errores del plano son Problem Details (RFC 9457).

#![allow(clippy::result_large_err)]

use axum::Extension;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::AuthedKey;
use crate::problem::{self, Problem};
use crate::rest::is_unique_violation;
use crate::state::AppState;
use crate::vault;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/credentials",
            post(crear).get(listar),
        )
        .route(
            "/credentials/{id}",
            get(obtener).patch(rotar).delete(eliminar),
        )
}

/// Metadatos de una credencial: jamás incluye el secreto.
#[derive(sqlx::FromRow, serde::Serialize)]
struct CredencialMeta {
    id: Uuid,
    name: String,
    description: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// Cuerpo de creación.
#[derive(Deserialize)]
struct CrearCredencial {
    name: String,
    /// Secreto en claro: write-only, cifrado en reposo y jamás devuelto.
    secret: String,
    #[serde(default)]
    description: Option<String>,
}

/// Cuerpo de rotación/actualización.
#[derive(Deserialize)]
struct PatchCredencial {
    /// Nuevo valor del secreto (rotación).
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

fn validar_name(name: &str) -> Result<(), Problem> {
    let name = name.trim();
    if name.is_empty() {
        return Err(problem::bad_request("el nombre de la credencial no puede estar vacío"));
    }
    if name.len() > 128 {
        return Err(problem::bad_request(
            "el nombre de la credencial no puede exceder 128 caracteres",
        ));
    }
    Ok(())
}

/// La clave de la fila tras un INSERT o UPDATE, para responder 409/404.
async fn fila_por_id(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<CredencialMeta, Problem> {
    sqlx::query_as::<_, CredencialMeta>(
        "SELECT id, name, description, created_at, updated_at \
         FROM credentials WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| problem::internal(format!("leer la credencial: {error}")))?
    .ok_or_else(|| problem::not_found("la credencial no existe en este workspace"))
}

async fn crear(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Json(cuerpo): Json<CrearCredencial>,
) -> Result<(StatusCode, Json<CredencialMeta>), Problem> {
    let workspace_id = key.workspace()?;

    let name = cuerpo.name.trim().to_string();
    validar_name(&name)?;

    let id = Uuid::now_v7();
    let cifrado = vault::encrypt(&cuerpo.secret);

    sqlx::query(
        "INSERT INTO credentials (id, workspace_id, name, secret_encrypted, description) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(&name)
    .bind(&cifrado)
    .bind(cuerpo.description.unwrap_or_default())
    .execute(&state.pool)
    .await
    .map_err(|error| {
        if is_unique_violation(&error) {
            problem::conflict(format!("ya existe una credencial llamada \"{name}\" en este workspace"))
        } else {
            problem::internal(format!("crear la credencial: {error}"))
        }
    })?;

    let fila = fila_por_id(&state.pool, workspace_id, id).await?;
    Ok((StatusCode::CREATED, Json(fila)))
}

async fn listar(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
) -> Result<Json<Value>, Problem> {
    let workspace_id = key.workspace()?;

    let credenciales: Vec<CredencialMeta> = sqlx::query_as(
        "SELECT id, name, description, created_at, updated_at \
         FROM credentials WHERE workspace_id = $1 ORDER BY name",
    )
    .bind(workspace_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("listar las credenciales: {error}")))?;

    Ok(Json(serde_json::json!({ "credentials": credenciales })))
}

async fn obtener(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(id): Path<Uuid>,
) -> Result<Json<CredencialMeta>, Problem> {
    let workspace_id = key.workspace()?;
    let fila = fila_por_id(&state.pool, workspace_id, id).await?;
    Ok(Json(fila))
}

async fn rotar(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(id): Path<Uuid>,
    Json(cuerpo): Json<PatchCredencial>,
) -> Result<Json<CredencialMeta>, Problem> {
    let workspace_id = key.workspace()?;

    // La rotación es el caso de uso central, pero permitir renombrar o
    // actualizar la descripción evita duplicar endpoints en MVP1.
    if let Some(name) = &cuerpo.name {
        validar_name(name)?;
    }
    if cuerpo.secret.is_none() && cuerpo.name.is_none() && cuerpo.description.is_none() {
        return Err(problem::bad_request(
            "indica al menos un campo a actualizar: secret, name o description",
        ));
    }

    // El UPDATE no toca el secreto si no viene; si viene, se re-cifra.
    let resultado = sqlx::query(
        "UPDATE credentials SET \
             name = COALESCE($3, name), \
             description = COALESCE($4, description), \
             secret_encrypted = COALESCE($5, secret_encrypted), \
             updated_at = now() \
         WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(cuerpo.name.clone())
    .bind(cuerpo.description.clone())
    .bind(cuerpo.secret.as_deref().map(vault::encrypt))
    .execute(&state.pool)
    .await
    .map_err(|error| {
        if is_unique_violation(&error) {
            problem::conflict(format!(
                "ya existe una credencial llamada \"{}\" en este workspace",
                cuerpo.name.clone().unwrap_or_default()
            ))
        } else {
            problem::internal(format!("rotar la credencial: {error}"))
        }
    })?;

    if resultado.rows_affected() == 0 {
        return Err(problem::not_found("la credencial no existe en este workspace"));
    }

    let fila = fila_por_id(&state.pool, workspace_id, id).await?;
    Ok(Json(fila))
}

async fn eliminar(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, Problem> {
    let workspace_id = key.workspace()?;

    let resultado = sqlx::query("DELETE FROM credentials WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|error| problem::internal(format!("eliminar la credencial: {error}")))?;

    if resultado.rows_affected() == 0 {
        return Err(problem::not_found("la credencial no existe en este workspace"));
    }
    Ok(StatusCode::NO_CONTENT)
}
