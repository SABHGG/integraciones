//! Bóveda de credenciales del plano admin (task 4.7).
//!
//! El secreto es **write-only**: se acepta en `POST` (creación) y `PATCH`
//! (rotación), se cifra en reposo con `vault::encrypt` y jamás se devuelve en
//! ninguna lectura — solo metadatos (`id`, `name`, `description`, fechas).
//! Los errores del plano son Problem Details (RFC 9457).

#![allow(clippy::result_large_err)]

use axum::Extension;
use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
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
        .route("/credentials", post(crear).get(listar))
        .route(
            "/credentials/{id}",
            get(obtener).patch(rotar).delete(eliminar),
        )
        .route("/credentials/{id}/versions", get(versiones))
}

/// Metadatos de una credencial: jamás incluye el secreto.
#[derive(sqlx::FromRow, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CredencialMeta {
    id: Uuid,
    name: String,
    description: String,
    /// Versión vigente que resuelve `ctx.credential(name)` (MVP3).
    current_version_id: Option<Uuid>,
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
        return Err(problem::bad_request(
            "el nombre de la credencial no puede estar vacío",
        ));
    }
    if name.len() > 128 {
        return Err(problem::bad_request(
            "el nombre de la credencial no puede exceder 128 caracteres",
        ));
    }
    Ok(())
}

/// La clave de la fila tras un INSERT o UPDATE, para responder 409/404.
async fn fila_por_id(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<CredencialMeta, Problem> {
    sqlx::query_as::<_, CredencialMeta>(
        "SELECT id, name, description, current_version_id, created_at, updated_at \
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

    // MVP3 (bóveda versionada): la creación escribe la versión 1 y la deja
    // vigente, en una sola transacción. La columna legada `secret_encrypted`
    // se conserva con el mismo valor por seguridad de rollback.
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| problem::internal(format!("crear la credencial: {error}")))?;
    sqlx::query(
        "INSERT INTO credentials (id, workspace_id, name, secret_encrypted, description) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(&name)
    .bind(&cifrado)
    .bind(cuerpo.description.unwrap_or_default())
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        if is_unique_violation(&error) {
            problem::conflict(format!(
                "ya existe una credencial llamada \"{name}\" en este workspace"
            ))
        } else {
            problem::internal(format!("crear la credencial: {error}"))
        }
    })?;
    let version_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO credential_versions (id, credential_id, secret_encrypted) \
         VALUES ($1, $2, $3)",
    )
    .bind(version_id)
    .bind(id)
    .bind(&cifrado)
    .execute(&mut *tx)
    .await
    .map_err(|error| problem::internal(format!("crear la versión 1: {error}")))?;
    sqlx::query("UPDATE credentials SET current_version_id = $2 WHERE id = $1")
        .bind(id)
        .bind(version_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| problem::internal(format!("fijar la versión vigente: {error}")))?;
    tx.commit()
        .await
        .map_err(|error| problem::internal(format!("crear la credencial: {error}")))?;

    let fila = fila_por_id(&state.pool, workspace_id, id).await?;
    Ok((StatusCode::CREATED, Json(fila)))
}

async fn listar(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
) -> Result<Json<Value>, Problem> {
    let workspace_id = key.workspace()?;

    let credenciales: Vec<CredencialMeta> = sqlx::query_as(
        "SELECT id, name, description, current_version_id, created_at, updated_at \
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

    // MVP3 (bóveda versionada): escribir el valor crea una versión INMUTABLE
    // y mueve la vigente; renombrar/descríbir no crea versión. En una sola
    // transacción para que la vigencia nunca quede a medias.
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| problem::internal(format!("rotar la credencial: {error}")))?;
    let nueva_version = if let Some(secreto) = &cuerpo.secret {
        let version_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO credential_versions (id, credential_id, secret_encrypted) \
             SELECT $1, id, $3 FROM credentials WHERE workspace_id = $2 AND id = $4",
        )
        .bind(version_id)
        .bind(workspace_id)
        .bind(vault::encrypt(secreto))
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|error| problem::internal(format!("crear la versión: {error}")))?;
        Some(version_id)
    } else {
        None
    };
    let resultado = sqlx::query(
        "UPDATE credentials SET \
             name = COALESCE($3, name), \
             description = COALESCE($4, description), \
             current_version_id = COALESCE($5, current_version_id), \
             updated_at = now() \
         WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(cuerpo.name.clone())
    .bind(cuerpo.description.clone())
    .bind(nueva_version)
    .execute(&mut *tx)
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
        return Err(problem::not_found(
            "la credencial no existe en este workspace",
        ));
    }
    tx.commit()
        .await
        .map_err(|error| problem::internal(format!("rotar la credencial: {error}")))?;

    let fila = fila_por_id(&state.pool, workspace_id, id).await?;
    Ok(Json(fila))
}

/// `GET /credentials/{id}/versions`: historial de versiones como METADATOS
/// (ids y fechas); jamás devuelve el valor de ninguna versión (write-only se
/// mantiene en todas). MVP3: rotación auditable.
async fn versiones(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, Problem> {
    let workspace_id = key.workspace()?;
    fila_por_id(&state.pool, workspace_id, id).await?;

    #[derive(sqlx::FromRow, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct VersionMeta {
        id: Uuid,
        rotated_at: chrono::DateTime<chrono::Utc>,
        created_at: chrono::DateTime<chrono::Utc>,
    }
    let versiones: Vec<VersionMeta> = sqlx::query_as(
        "SELECT v.id, v.rotated_at, v.created_at \
         FROM credential_versions v \
         JOIN credentials c ON c.id = v.credential_id \
         WHERE c.workspace_id = $1 AND c.id = $2 \
         ORDER BY v.rotated_at, v.created_at",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("listar las versiones: {error}")))?;

    Ok(Json(serde_json::json!({ "versions": versiones })))
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
        return Err(problem::not_found(
            "la credencial no existe en este workspace",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}
