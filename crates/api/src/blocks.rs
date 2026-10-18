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
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::AuthedKey;
use crate::cursor;
use crate::problem::{self, Problem};
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

/// Página por defecto de los listados.
const DEFAULT_LIMIT: i64 = 20;
/// Tope duro de la página: el cliente puede pedir menos, nunca más.
const MAX_LIMIT: i64 = 100;

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

/// Envelope común de listados paginados por cursor.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Page<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
}

/// Query string de los listados. `limit` se recibe como texto para poder
/// responder 400 Problem Details en lugar del 400 en texto plano de axum.
#[derive(Debug, Deserialize)]
struct PageQuery {
    limit: Option<String>,
    cursor: Option<String>,
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
    let id = parse_id(&id)?;
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
    let id = parse_id(&id)?;
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
    code: String,
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
    let id = parse_id(&id)?;
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
        return Err(problem::not_found(
            "el bloque no existe en este workspace",
        ));
    };

    if block_type != "custom_logic" {
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

    // TypeScript → JavaScript: un error de compilación no crea versión.
    let js = transpile::transpile_typescript(&request.code).map_err(|error| match error {
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
    })?;

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
    let id = parse_id(&id)?;
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
fn parse_body<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, Problem> {
    serde_json::from_slice(body).map_err(|error| {
        invalid_body(
            "el cuerpo de la petición no cumple el contrato",
            vec![field_error(
                &field_from_serde_error(&error),
                error.to_string(),
            )],
        )
    })
}

/// Nombre del campo culpable a partir del mensaje de `serde_json`.
///
/// `serde_json` no expone la ruta del error de forma estructurada; los tres
/// mensajes que sí la nombran son `missing field`, `unknown field` y
/// `duplicate field`. El resto (tipos, sintaxis) cae a `"body"`.
fn field_from_serde_error(error: &serde_json::Error) -> String {
    let message = error.to_string();
    for marker in ["missing field `", "unknown field `", "duplicate field `"] {
        if let Some(rest) = message.split(marker).nth(1)
            && let Some(field) = rest.split('`').next()
            && !field.is_empty()
        {
            return field.to_string();
        }
    }
    "body".to_string()
}

/// Un 422 con la extensión `errors` (lista de `{field, message, …}`).
fn invalid_body(detail: impl Into<String>, errors: Vec<Value>) -> Problem {
    problem::unprocessable_entity(detail).with_extension("errors", Value::Array(errors))
}

/// Entrada de `errors` con el campo señalado.
fn field_error(field: &str, message: impl Into<String>) -> Value {
    json!({ "field": field, "message": message.into() })
}

/// Valida el `limit` de la query string: `1..=MAX_LIMIT`, default [`DEFAULT_LIMIT`].
fn page_limit(raw: Option<&str>) -> Result<i64, Problem> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_LIMIT);
    };
    let limit: i64 = raw.trim().parse().map_err(|_| {
        problem::bad_request(format!(
            "limit debe ser un entero entre 1 y {MAX_LIMIT} (se recibió \"{raw}\")"
        ))
    })?;
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(problem::bad_request(format!(
            "limit debe estar entre 1 y {MAX_LIMIT} (se recibió {limit})"
        )));
    }
    Ok(limit)
}

/// Recorta la página a `limit` y calcula el cursor de la siguiente si sobró
/// exactamente la fila de sondeo.
fn next_cursor<T>(items: &mut Vec<T>, limit: i64, encode: impl Fn(&T) -> String) -> Option<String> {
    if items.len() as i64 <= limit {
        return None;
    }
    items.truncate(limit as usize);
    items.last().map(encode)
}

/// Slugs: minúsculas alfanuméricas en grupos separados por un único `-`.
fn check_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() || slug.chars().count() > 64 {
        return Err("el slug debe tener entre 1 y 64 caracteres".to_string());
    }
    let valid = !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        return Err(format!(
            "el slug \"{slug}\" no es válido: solo minúsculas, dígitos y guiones simples \
             (p.ej. http-get)"
        ));
    }
    Ok(())
}

/// Texto obligatorio: no vacío (ignorando espacios) y con tope de longitud.
fn check_text(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} no puede estar vacío"));
    }
    if value.chars().count() > max {
        return Err(format!("{field} no puede superar los {max} caracteres"));
    }
    Ok(())
}

/// Interpreta el `{id}` de la ruta. Un id no-UUID es 404 (no existe tal recurso),
/// no el 400 en texto plano que daría un extractor `Path<Uuid>`.
fn parse_id(raw: &str) -> Result<Uuid, Problem> {
    Uuid::parse_str(raw).map_err(|_| problem::not_found("el bloque no existe en este workspace"))
}

/// `true` si el error es una violación de unicidad de Postgres (SQLSTATE 23505).
fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|db| db.is_unique_violation())
}

/// 500 Problem Details con el detalle técnico solo en los logs.
fn db_error(context: &str, error: sqlx::Error) -> Problem {
    tracing::error!(error = %error, contexto = context, "fallo de base de datos");
    problem::internal(format!("fallo de base de datos al {context}"))
}

/// 201 con `Location` del recurso recién creado.
fn created(location: &str, body: Json<impl Serialize>) -> Response {
    let mut response = body.into_response();
    *response.status_mut() = StatusCode::CREATED;
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(location).expect("la ruta del recurso es un header válido"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_slugs_validos_son_minusculas_con_guiones_simples() {
        for slug in ["a", "http-get", "bloque-2", "x1-y2-z3"] {
            assert!(check_slug(slug).is_ok(), "{slug} debería ser válido");
        }
        for slug in [
            "",
            "Con-Mayusculas",
            "con espacio",
            "guion-final-",
            "-guion-inicial",
            "doble--guion",
            "acentué",
            "emoji🙂",
            "punto.y.coma",
        ] {
            assert!(check_slug(slug).is_err(), "{slug} debería rechazarse");
        }
        assert!(check_slug(&"a".repeat(64)).is_ok());
        assert!(check_slug(&"a".repeat(65)).is_err());
    }

    #[test]
    fn el_limit_por_defecto_y_los_fuera_de_rango() {
        assert_eq!(page_limit(None).unwrap(), DEFAULT_LIMIT);
        assert_eq!(page_limit(Some("1")).unwrap(), 1);
        assert_eq!(page_limit(Some("100")).unwrap(), 100);
        for raw in ["0", "101", "-1", "abc", ""] {
            let problem = page_limit(Some(raw)).expect_err(raw);
            assert_eq!(problem.status, StatusCode::BAD_REQUEST);
        }
    }

    #[test]
    fn el_campo_culpable_se_extrae_del_error_de_serde() {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        #[allow(dead_code)]
        struct Demo {
            slug: String,
        }

        let missing = serde_json::from_str::<Demo>("{}").expect_err("falta slug");
        assert_eq!(field_from_serde_error(&missing), "slug");

        let unknown = serde_json::from_str::<Demo>(r#"{"slug":"x","raro":1}"#)
            .expect_err("campo desconocido");
        assert_eq!(field_from_serde_error(&unknown), "raro");

        let wrong_type = serde_json::from_str::<Demo>(r#"{"slug":42}"#).expect_err("tipo");
        assert_eq!(field_from_serde_error(&wrong_type), "body");

        let syntax = serde_json::from_str::<Demo>("{").expect_err("json roto");
        assert_eq!(field_from_serde_error(&syntax), "body");
    }

    #[test]
    fn el_cursor_de_la_pagina_solo_aparece_si_sobra_la_fila_de_sondeo() {
        let mut justos = vec![3, 2];
        assert_eq!(next_cursor(&mut justos, 2, |v| v.to_string()), None);
        assert_eq!(justos, vec![3, 2]);

        let mut sobran = vec![3, 2, 1];
        assert_eq!(
            next_cursor(&mut sobran, 2, |v| v.to_string()).as_deref(),
            Some("2")
        );
        assert_eq!(sobran, vec![3, 2], "la fila de sondeo se descarta");
    }

    #[test]
    fn el_422_lleva_los_errores_por_campo() {
        let problem = invalid_body("contrato", vec![field_error("slug", "inválido")]);
        assert_eq!(problem.status, StatusCode::UNPROCESSABLE_ENTITY);
        let body = problem.to_json(None);
        assert_eq!(body["errors"][0]["field"], "slug");
        assert_eq!(body["errors"][0]["message"], "inválido");
    }
}
