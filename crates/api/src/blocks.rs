//! API REST de la biblioteca de bloques (spec.md `block-library`).
//!
//! Todas las rutas cuelgan del plano de administración (`/api/v1`) y por tanto
//! llegan aquí ya autenticadas con una [`AuthedKey`]; cada handler exige además
//! que la key tenga workspace ([`AuthedKey::workspace`]) y limita todas las
//! consultas a ese workspace. Un recurso de otro workspace es indistinguible de
//! uno inexistente: 404, nunca 403 (no se filtra la existencia de ids ajenos).
//!
//! Decisiones de esta tarea:
//! - **Publicar código en un bloque `http` es 422**, no 409: el conflicto no es
//!   de estado de la base (no hay carrera ni duplicado) sino que el cuerpo no
//!   cumple el contrato del recurso — un bloque built-in `http` no lleva código
//!   propio, su comportamiento lo define el ejecutor. Un 409 invitaría al
//!   cliente a reintentar con otro payload, y aquí ningún payload lo arregla
//!   salvo crear un bloque `custom_logic`.
//! - El `code` guardado es **siempre JavaScript transpilado** (`block_versions.code`);
//!   el TypeScript del usuario no se persiste: es una fuente, no un artefacto.
//! - Las versiones son inmutables: publicar crea fila nueva con
//!   `version = COALESCE(MAX(version), 0) + 1` dentro de una transacción que
//!   bloquea la fila del bloque (`FOR UPDATE`), lo que serializa publicaciones
//!   concurrentes del mismo bloque en lugar de chocar contra
//!   `UNIQUE (block_id, version)`.
//
// Los handlers devuelven `Result<_, Problem>`: `Problem` ocupa ~150 bytes (5
// campos con cadenas y opcionales) y clippy lo señala, pero solo se construye en
// la ruta de error y boxearlo tocaría la API pública de `problem.rs` sin
// ganancia. Mismo criterio que en `auth::require_api_key`.
#![allow(clippy::result_large_err)]

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use axum::{Extension, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::AuthedKey;
use crate::cursor;
use crate::problem::{self, Problem};
use crate::rest::{
    Page, PageQuery, check_slug, check_text, created, db_error, field_error, invalid_body,
    is_unique_violation, next_cursor, page_limit, parse_body, parse_id,
};
use crate::state::AppState;
use crate::transpile::{self, TranspileError};
use crate::ui;

/// Rutas de la biblioteca de bloques (se montan bajo `/api/v1`).
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/blocks", get(list_blocks).post(create_block))
        .route("/blocks/{id}", get(get_block).delete(delete_block))
        .route(
            "/blocks/{id}/versions",
            get(list_versions).post(publish_version),
        )
}

/// Columnas de `blocks` en el orden de [`BlockDto`].
const BLOCK_COLUMNS: &str = "id, workspace_id, slug, name, block_type, category, \
                             description, icon, latest_version, created_at, updated_at";

/// Columnas de `block_versions` en el orden de [`BlockVersionDto`].
const VERSION_COLUMNS: &str = "id, block_id, version, code, config_schema, input_schema, \
                               output_schema, ui, created_at";

/// Bloque tal como lo expone la API.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct BlockDto {
    id: Uuid,
    workspace_id: Uuid,
    slug: String,
    name: String,
    block_type: String,
    category: String,
    description: String,
    icon: String,
    latest_version: Option<i32>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Versión inmutable de un bloque. `code` es el JS transpilado (NULL solo para
/// filas históricas de bloques built-in).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct BlockVersionDto {
    id: Uuid,
    block_id: Uuid,
    version: i32,
    code: Option<String>,
    config_schema: Value,
    input_schema: Value,
    output_schema: Value,
    ui: Value,
    created_at: DateTime<Utc>,
}

// ── POST /blocks ───────────────────────────────────────────────────────

/// Cuerpo de `POST /blocks`. `deny_unknown_fields` convierte una clave de más en
/// 422 en vez de ignorarla en silencio.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateBlockRequest {
    slug: String,
    name: String,
    block_type: String,
    category: Option<String>,
    description: Option<String>,
    icon: Option<String>,
}

/// Crea un bloque en la biblioteca del workspace de la key.
async fn create_block(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    body: Bytes,
) -> Result<Response, Problem> {
    let workspace_id = key.workspace()?;
    let request: CreateBlockRequest = parse_body(&body)?;

    let mut errors = Vec::new();
    if let Err(message) = check_slug(&request.slug) {
        errors.push(field_error("slug", message));
    }
    if let Err(message) = check_text("name", &request.name, 200) {
        errors.push(field_error("name", message));
    }
    if !matches!(request.block_type.as_str(), "http" | "custom_logic") {
        errors.push(field_error(
            "blockType",
            format!(
                "el blockType debe ser \"http\" o \"custom_logic\" (se recibió \"{}\")",
                request.block_type
            ),
        ));
    }
    if let Some(category) = &request.category
        && let Err(message) = check_text("category", category, 64)
    {
        errors.push(field_error("category", message));
    }
    if let Some(description) = &request.description
        && description.chars().count() > 2_000
    {
        errors.push(field_error(
            "description",
            "la description no puede superar los 2000 caracteres",
        ));
    }
    if let Some(icon) = &request.icon
        && let Err(message) = check_text("icon", icon, 64)
    {
        errors.push(field_error("icon", message));
    }
    if !errors.is_empty() {
        return Err(invalid_body("el bloque no cumple el contrato", errors));
    }

    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, BlockDto>(&format!(
        "INSERT INTO blocks (id, workspace_id, slug, name, block_type, category, description, icon)
         VALUES ($1, $2, $3, $4, $5, COALESCE($6, 'general'), COALESCE($7, ''), COALESCE($8, 'box'))
         RETURNING {BLOCK_COLUMNS}"
    ))
    .bind(id)
    .bind(workspace_id)
    .bind(&request.slug)
    .bind(&request.name)
    .bind(&request.block_type)
    .bind(request.category.as_deref())
    .bind(request.description.as_deref())
    .bind(request.icon.as_deref())
    .fetch_one(&state.pool)
    .await;

    match row {
        Ok(block) => Ok(created(
            &format!("/api/v1/blocks/{}", block.id),
            Json(block),
        )),
        // UNIQUE (workspace_id, slug): solo el duplicado se traduce a 409.
        Err(error) if is_unique_violation(&error) => Err(problem::conflict(format!(
            "ya existe un bloque con el slug \"{}\" en este workspace",
            request.slug
        ))),
        Err(error) => Err(db_error("crear el bloque", error)),
    }
}

// ── GET /blocks ────────────────────────────────────────────────────────

/// Lista los bloques del workspace, del más nuevo al más viejo, con paginación
/// keyset por cursor sobre `(created_at, id)`.
async fn list_blocks(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<BlockDto>>, Problem> {
    let workspace_id = key.workspace()?;
    let limit = page_limit(query.limit.as_deref())?;
    let after = match query.cursor.as_deref() {
        Some(raw) => Some(
            cursor::decode_block_cursor(raw)
                .map_err(|error| problem::bad_request(error.to_string()))?,
        ),
        None => None,
    };
    let (after_created_at, after_id) = match after {
        Some((created_at, id)) => (Some(created_at), Some(id)),
        None => (None, None),
    };

    // Se pide una fila de más para saber si hay página siguiente sin contar.
    let mut items = sqlx::query_as::<_, BlockDto>(&format!(
        "SELECT {BLOCK_COLUMNS} FROM blocks
         WHERE workspace_id = $1
           AND ($2::timestamptz IS NULL OR (created_at, id) < ($2::timestamptz, $3::uuid))
         ORDER BY created_at DESC, id DESC
         LIMIT $4"
    ))
    .bind(workspace_id)
    .bind(after_created_at)
    .bind(after_id)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await
    .map_err(|error| db_error("listar los bloques", error))?;

    let next_cursor = next_cursor(&mut items, limit, |block| {
        cursor::encode_block_cursor(block.created_at, block.id)
    });

    Ok(Json(Page { items, next_cursor }))
}

// ── GET/DELETE /blocks/{id} ────────────────────────────────────────────

/// Devuelve un bloque del workspace.
async fn get_block(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(id): Path<String>,
) -> Result<Json<BlockDto>, Problem> {
    let workspace_id = key.workspace()?;
    let id = parse_id(&id, "el bloque no existe en este workspace")?;
    let block = load_block(&state, workspace_id, id).await?;
    Ok(Json(block))
}

/// Borra un bloque sin versiones referenciadas por ningún puente del workspace.
async fn delete_block(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(id): Path<String>,
) -> Result<StatusCode, Problem> {
    let workspace_id = key.workspace()?;
    let id = parse_id(&id, "el bloque no existe en este workspace")?;
    // 404 si no existe o es de otro workspace (no se filtra su existencia).
    let block = load_block(&state, workspace_id, id).await?;

    // La referencia vive dentro del JSONB de la definición del puente, no en una
    // FK: se busca `definition.blocks[*].blockId`. Solo cuentan los puentes del
    // mismo workspace (una definición ajena es una referencia colgante y no debe
    // impedir el borrado).
    let referencing_bridge: Option<String> = sqlx::query_scalar(
        "SELECT b.slug
         FROM bridge_versions bv
         JOIN bridges b ON b.id = bv.bridge_id
         WHERE b.workspace_id = $1
           AND jsonb_typeof(bv.definition -> 'blocks') = 'array'
           AND EXISTS (
               SELECT 1
               FROM jsonb_array_elements(bv.definition -> 'blocks') AS elem
               WHERE elem ->> 'blockId' = $2
           )
         ORDER BY b.slug
         LIMIT 1",
    )
    .bind(workspace_id)
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| db_error("comprobar las referencias del bloque", error))?;

    if let Some(bridge_slug) = referencing_bridge {
        // Sin FK que lo impida, esta comprobación y el DELETE no son atómicos:
        // un puente publicado en el mismo instante podría quedar colgando. En
        // MVP1 el riesgo es aceptable (una sola escritura de puentes a la vez) y
        // se prefiere 409 explicativo a un borrado silencioso.
        return Err(problem::conflict(format!(
            "el bloque \"{}\" está referenciado por versiones del puente \"{bridge_slug}\" \
             y no se puede borrar; elimina esa versión del puente o publica una que no lo use",
            block.slug
        ))
        .with_extension("bridgeSlug", json!(bridge_slug)));
    }

    sqlx::query("DELETE FROM blocks WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(workspace_id)
        .execute(&state.pool)
        .await
        .map_err(|error| db_error("borrar el bloque", error))?;

    Ok(StatusCode::NO_CONTENT)
}

// ── POST /blocks/{id}/versions ─────────────────────────────────────────

/// Cuerpo de `POST /blocks/{id}/versions`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishVersionRequest {
    /// TypeScript del bloque (`export default function (input, ctx) { … }`).
    /// Solo para `custom_logic`: los built-in http publican sin código.
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    config_schema: Option<Value>,
    #[serde(default)]
    input_schema: Option<Value>,
    #[serde(default)]
    output_schema: Option<Value>,
    #[serde(default)]
    ui: Option<Value>,
}

/// Publica una versión nueva e inmutable de un bloque `custom_logic`.
async fn publish_version(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Problem> {
    let workspace_id = key.workspace()?;
    let id = parse_id(&id, "el bloque no existe en este workspace")?;
    let request: PublishVersionRequest = parse_body(&body)?;

    // Bloqueo de la fila del bloque: serializa publicaciones concurrentes del
    // mismo bloque (el `MAX(version) + 1` deja de tener carreras) y a la vez
    // comprueba que el bloque existe en el workspace.
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| db_error("abrir la transacción de publicación", error))?;

    let block_type: Option<String> = sqlx::query_scalar(
        "SELECT block_type FROM blocks WHERE id = $1 AND workspace_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| db_error("buscar el bloque a publicar", error))?;

    let Some(block_type) = block_type else {
        return Err(problem::not_found("el bloque no existe en este workspace"));
    };

    // El código depende del tipo: custom_logic lo exige (el JS es su
    // comportamiento); los built-in http lo prohíben (lo define el ejecutor)
    // y publican versiones de solo schemas con `code` NULL.
    let js: Option<String> = if block_type == "custom_logic" {
        let Some(code) = &request.code else {
            return Err(invalid_body(
                "los bloques custom_logic necesitan código publicado",
                vec![field_error(
                    "code",
                    "un bloque custom_logic publica siempre su TypeScript: falta \"code\"",
                )],
            ));
        };
        // TypeScript → JavaScript: un error de compilación no crea versión.
        Some(
            transpile::transpile_typescript(code).map_err(|error| match error {
                TranspileError::Syntax {
                    line,
                    column,
                    message,
                } => invalid_body(
                    "el código del bloque no compila",
                    vec![json!({
                        "field": "code",
                        "message": message,
                        "line": line,
                        "column": column,
                    })],
                ),
                TranspileError::MissingDefaultExport => invalid_body(
                    "el código del bloque no cumple el contrato",
                    vec![field_error("code", error.to_string())],
                ),
                TranspileError::Codegen(message) => problem::internal(format!(
                    "no se pudo generar el JavaScript del bloque: {message}"
                )),
            })?,
        )
    } else {
        if request.code.is_some() {
            return Err(invalid_body(
                "solo los bloques custom_logic llevan código publicado",
                vec![field_error(
                    "blockType",
                    format!(
                        "el bloque es de tipo \"{block_type}\": su comportamiento lo define el \
                         ejecutor y no acepta código propio (crea un bloque custom_logic para \
                         publicar TypeScript)"
                    ),
                )],
            ));
        }
        None
    };

    // Los tres schemas deben pertenecer al subconjunto estricto de design.md.
    let config_schema = request.config_schema.unwrap_or_else(|| json!({}));
    let input_schema = request.input_schema.unwrap_or_else(|| json!({}));
    let output_schema = request.output_schema.unwrap_or_else(|| json!({}));
    let ui_doc = request.ui.unwrap_or_else(|| json!({}));

    let mut errors = Vec::new();
    for (field, schema) in [
        ("configSchema", &config_schema),
        ("inputSchema", &input_schema),
        ("outputSchema", &output_schema),
    ] {
        if let Err(error) = domain::schema::validate_schema_doc(schema) {
            errors.push(field_error(field, error.to_string()));
        }
    }
    if let Err(error) = ui::validate(&ui_doc) {
        errors.push(field_error("ui", error.to_string()));
    }
    if !errors.is_empty() {
        return Err(invalid_body(
            "los metadatos de la versión no cumplen el contrato",
            errors,
        ));
    }

    let version_id = Uuid::now_v7();
    let version = sqlx::query_as::<_, BlockVersionDto>(&format!(
        "WITH siguiente AS (
             SELECT COALESCE(MAX(version), 0) + 1 AS version
             FROM block_versions WHERE block_id = $2
         )
         INSERT INTO block_versions
             (id, block_id, version, code, config_schema, input_schema, output_schema, ui)
         SELECT $1, $2, siguiente.version, $3, $4, $5, $6, $7 FROM siguiente
         RETURNING {VERSION_COLUMNS}"
    ))
    .bind(version_id)
    .bind(id)
    .bind(&js)
    .bind(&config_schema)
    .bind(&input_schema)
    .bind(&output_schema)
    .bind(&ui_doc)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| db_error("insertar la versión del bloque", error))?;

    sqlx::query("UPDATE blocks SET latest_version = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(version.version)
        .execute(&mut *tx)
        .await
        .map_err(|error| db_error("actualizar latest_version del bloque", error))?;

    tx.commit()
        .await
        .map_err(|error| db_error("confirmar la publicación", error))?;

    Ok(created(
        &format!("/api/v1/blocks/{id}/versions/{}", version.version),
        Json(version),
    ))
}

// ── GET /blocks/{id}/versions ──────────────────────────────────────────

/// Lista las versiones del bloque, de la más nueva a la más vieja.
async fn list_versions(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<BlockVersionDto>>, Problem> {
    let workspace_id = key.workspace()?;
    let id = parse_id(&id, "el bloque no existe en este workspace")?;
    let limit = page_limit(query.limit.as_deref())?;
    let after_version = match query.cursor.as_deref() {
        Some(raw) => Some(
            cursor::decode_version_cursor(raw)
                .map_err(|error| problem::bad_request(error.to_string()))?,
        ),
        None => None,
    };

    // 404 antes de listar: un listado vacío no debe revelar si el id existe.
    load_block(&state, workspace_id, id).await?;

    let mut items = sqlx::query_as::<_, BlockVersionDto>(&format!(
        "SELECT {VERSION_COLUMNS} FROM block_versions
         WHERE block_id = $1
           AND ($2::int IS NULL OR version < $2::int)
         ORDER BY version DESC
         LIMIT $3"
    ))
    .bind(id)
    .bind(after_version)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await
    .map_err(|error| db_error("listar las versiones del bloque", error))?;

    let next_cursor = next_cursor(&mut items, limit, |version| {
        cursor::encode_version_cursor(version.version)
    });

    Ok(Json(Page { items, next_cursor }))
}

// ── Utilidades ─────────────────────────────────────────────────────────

/// Carga un bloque del workspace o 404 (mismo mensaje para inexistente y ajeno).
async fn load_block(state: &AppState, workspace_id: Uuid, id: Uuid) -> Result<BlockDto, Problem> {
    sqlx::query_as::<_, BlockDto>(&format!(
        "SELECT {BLOCK_COLUMNS} FROM blocks WHERE id = $1 AND workspace_id = $2"
    ))
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| db_error("buscar el bloque", error))?
    .ok_or_else(|| problem::not_found("el bloque no existe en este workspace"))
}

/// Deserializa el cuerpo crudo para responder 422 Problem Details (con el campo
/// culpable cuando serde lo nombra) en lugar del 400 en texto plano de axum.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_id_que_no_es_uuid_es_un_404_con_el_mensaje_del_recurso() {
        let problem = parse_id("no-es-uuid", "el bloque no existe en este workspace")
            .expect_err("id inválido");
        assert_eq!(problem.status, StatusCode::NOT_FOUND);
        assert!(
            problem.detail.unwrap().contains("bloque"),
            "el 404 nombra el recurso"
        );
    }
}
