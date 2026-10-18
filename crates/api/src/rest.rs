//! Utilidades compartidas por los recursos del plano de administración:
//! envelope de paginación por cursor, parseo de cuerpos e ids con Problem
//! Details, validaciones de texto y traducción de errores de base de datos.
//!
//! Nació al construir la API de puentes (tarea 4.2): bloques y puentes son
//! listados paginados con la misma forma (`{items, nextCursor}`), el mismo tope
//! de página y el mismo criterio de errores, así que las piezas viven aquí en
//! lugar de duplicarse por recurso.

use axum::Json;
use axum::body::Bytes;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::problem::{self, Problem};

/// Página por defecto de los listados.
pub const DEFAULT_LIMIT: i64 = 20;
/// Tope duro de la página: el cliente puede pedir menos, nunca más.
pub const MAX_LIMIT: i64 = 100;

/// Envelope común de listados paginados por cursor.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    /// Elementos de la página.
    pub items: Vec<T>,
    /// Cursor opaco de la página siguiente (`None` en la última).
    pub next_cursor: Option<String>,
}

/// Query string de los listados. `limit` se recibe como texto para poder
/// responder 400 Problem Details en lugar del 400 en texto plano de axum.
#[derive(Debug, Deserialize)]
pub struct PageQuery {
    /// Tamaño de página pedido (`1..=MAX_LIMIT`).
    pub limit: Option<String>,
    /// Cursor opaco de la página siguiente.
    pub cursor: Option<String>,
}

/// Valida el `limit` de la query string: `1..=MAX_LIMIT`, default [`DEFAULT_LIMIT`].
///
/// `Problem` es grande pero solo se construye en la ruta de error.
#[allow(clippy::result_large_err)]
pub fn page_limit(raw: Option<&str>) -> Result<i64, Problem> {
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
pub fn next_cursor<T>(items: &mut Vec<T>, limit: i64, encode: impl Fn(&T) -> String) -> Option<String> {
    if items.len() as i64 <= limit {
        return None;
    }
    items.truncate(limit as usize);
    items.last().map(encode)
}

/// Deserializa el cuerpo crudo para responder 422 Problem Details (con el campo
/// culpable cuando serde lo nombra) en lugar del 400 en texto plano de axum.
#[allow(clippy::result_large_err)]
pub fn parse_body<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, Problem> {
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
pub fn field_from_serde_error(error: &serde_json::Error) -> String {
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
pub fn invalid_body(detail: impl Into<String>, errors: Vec<Value>) -> Problem {
    problem::unprocessable_entity(detail).with_extension("errors", Value::Array(errors))
}

/// Entrada de `errors` con el campo señalado.
pub fn field_error(field: &str, message: impl Into<String>) -> Value {
    serde_json::json!({ "field": field, "message": message.into() })
}

/// Interpreta el `{id}` de la ruta. Un id no-UUID es 404 (no existe tal
/// recurso), no el 400 en texto plano que daría un extractor `Path<Uuid>`.
#[allow(clippy::result_large_err)]
pub fn parse_id(raw: &str, not_found: &str) -> Result<Uuid, Problem> {
    Uuid::parse_str(raw).map_err(|_| problem::not_found(not_found))
}

/// `true` si el error es una violación de unicidad de Postgres (SQLSTATE 23505).
pub fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|db| db.is_unique_violation())
}

/// 500 Problem Details con el detalle técnico solo en los logs.
pub fn db_error(context: &str, error: sqlx::Error) -> Problem {
    tracing::error!(error = %error, contexto = context, "fallo de base de datos");
    problem::internal(format!("fallo de base de datos al {context}"))
}

/// 201 con `Location` del recurso recién creado.
pub fn created(location: &str, body: Json<impl Serialize>) -> Response {
    let mut response = body.into_response();
    *response.status_mut() = StatusCode::CREATED;
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(location).expect("la ruta del recurso es un header válido"),
    );
    response
}

/// Slugs: minúsculas alfanuméricas en grupos separados por un único `-`.
pub fn check_slug(slug: &str) -> Result<(), String> {
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
pub fn check_text(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} no puede estar vacío"));
    }
    if value.chars().count() > max {
        return Err(format!("{field} no puede superar los {max} caracteres"));
    }
    Ok(())
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

    #[test]
    fn check_text_rechaza_vacio_y_demasiado_largo() {
        assert!(check_text("name", "bloque", 10).is_ok());
        assert!(check_text("name", "   ", 10).is_err());
        assert!(check_text("name", &"a".repeat(11), 10).is_err());
    }
}
