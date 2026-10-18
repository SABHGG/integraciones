//! Contrato de metadatos de presentación (`ui`) de una versión de bloque.
//!
//! Decisión de design.md: "Convención de metadatos de presentación (`ui`: title,
//! orden, widget, help) junto a cada schema desde el día 1, y formato especial
//! para strings con interpolación de refs (`ref-template`)". El generador de
//! formularios del MVP3 consume estos metadatos, así que el contrato se valida
//! **al publicar** (un `ui` inválido es un 422, nunca un dato corrupto en BD).
//!
//! Contrato aceptado (todas las claves opcionales, ninguna desconocida):
//! - `title`: string no vacío — título del formulario del bloque.
//! - `order`: array de nombres de campo únicos y no vacíos — orden de pintado.
//! - `widget`: objeto `campo → id de widget`; el id es kebab-case en minúsculas
//!   (`text`, `textarea`, `select`, `json`, …) y se reserva `ref-template` para
//!   los strings con interpolación de refs `{$.ref}`.
//! - `help`: objeto `campo → texto de ayuda` no vacío.
//!
//! El conjunto de claves es cerrado a propósito, igual que el subconjunto de
//! JSON Schema: un `ui` con claves libres no sería determinista para el
//! generador de formularios.

use serde_json::Value;

/// Violación concreta del contrato de `ui`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("ui inválido en {path}: {message}")]
pub struct UiError {
    /// Ruta dentro del documento `ui` (`$.title`, `$.widget.total`, …).
    pub path: String,
    /// Qué se esperaba y qué se encontró.
    pub message: String,
}

/// Widget reservado para strings con interpolación de refs (`{$.ref}`).
pub const REF_TEMPLATE_WIDGET: &str = "ref-template";

/// Claves admitidas en el documento `ui`.
const ALLOWED_KEYS: [&str; 4] = ["title", "order", "widget", "help"];

/// Valida un documento `ui` completo (el que guarda `block_versions.ui`).
///
/// Devuelve la primera violación encontrada, con la ruta exacta del documento.
pub fn validate(ui: &Value) -> Result<(), UiError> {
    let Value::Object(map) = ui else {
        return Err(invalid("$", "el ui debe ser un objeto JSON"));
    };

    for (key, value) in map {
        match key.as_str() {
            "title" => check_title(value)?,
            "order" => check_order(value)?,
            "widget" => check_widget_map(value)?,
            "help" => check_help_map(value)?,
            other => {
                return Err(invalid(
                    &format!("$.{other}"),
                    format!(
                        "la clave \"{other}\" no pertenece al contrato de ui (admitidas: {})",
                        ALLOWED_KEYS.join(", ")
                    ),
                ));
            }
        }
    }

    Ok(())
}

/// `title` debe ser un string no vacío.
fn check_title(value: &Value) -> Result<(), UiError> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Ok(()),
        Value::String(_) => Err(invalid("$.title", "el title no puede estar vacío")),
        other => Err(invalid(
            "$.title",
            format!("el title debe ser un string (se encontró {})", describe(other)),
        )),
    }
}

/// `order` debe ser un array de nombres de campo únicos y no vacíos.
fn check_order(value: &Value) -> Result<(), UiError> {
    let Value::Array(items) = value else {
        return Err(invalid(
            "$.order",
            format!(
                "order debe ser un array de nombres de campo (se encontró {})",
                describe(value)
            ),
        ));
    };
    if items.is_empty() {
        return Err(invalid("$.order", "order no puede estar vacío"));
    }

    let mut seen: Vec<&str> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let Value::String(name) = item else {
            return Err(invalid(
                &format!("$.order.{index}"),
                format!("cada entrada de order debe ser un string (se encontró {})", describe(item)),
            ));
        };
        if name.trim().is_empty() {
            return Err(invalid(
                &format!("$.order.{index}"),
                "una entrada de order no puede estar vacía",
            ));
        }
        if seen.contains(&name.as_str()) {
            return Err(invalid(
                &format!("$.order.{index}"),
                format!("el campo \"{name}\" está repetido en order"),
            ));
        }
        seen.push(name.as_str());
    }

    Ok(())
}

/// `widget` mapea campo → id de widget.
fn check_widget_map(value: &Value) -> Result<(), UiError> {
    let Value::Object(map) = value else {
        return Err(invalid(
            "$.widget",
            format!(
                "widget debe ser un objeto campo → widget (se encontró {})",
                describe(value)
            ),
        ));
    };

    for (field, widget) in map {
        let path = format!("$.widget.{field}");
        if field.trim().is_empty() {
            return Err(invalid(&path, "el campo de widget no puede estar vacío"));
        }
        let Some(id) = widget.as_str() else {
            return Err(invalid(
                &path,
                format!("el id de widget debe ser un string (se encontró {})", describe(widget)),
            ));
        };
        if !is_widget_id(id) {
            return Err(invalid(
                &path,
                format!(
                    "el id de widget \"{id}\" no es válido: se espera kebab-case \
                     (p.ej. text, textarea, ref-template)"
                ),
            ));
        }
    }

    Ok(())
}

/// `help` mapea campo → texto de ayuda.
fn check_help_map(value: &Value) -> Result<(), UiError> {
    let Value::Object(map) = value else {
        return Err(invalid(
            "$.help",
            format!(
                "help debe ser un objeto campo → texto (se encontró {})",
                describe(value)
            ),
        ));
    };

    for (field, text) in map {
        let path = format!("$.help.{field}");
        match text {
            Value::String(help) if !help.trim().is_empty() => {}
            Value::String(_) => {
                return Err(invalid(&path, "el texto de ayuda no puede estar vacío"));
            }
            other => {
                return Err(invalid(
                    &path,
                    format!("el texto de ayuda debe ser un string (se encontró {})", describe(other)),
                ));
            }
        }
    }

    Ok(())
}

/// `true` si el id es kebab-case en minúsculas (`text`, `ref-template`, `key-value`).
fn is_widget_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Tipo JSON legible para los mensajes de error.
fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Constructor de una violación con su ruta.
fn invalid(path: &str, message: impl Into<String>) -> UiError {
    UiError {
        path: path.to_string(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assert_invalid(ui: &Value, needle: &str) {
        let err = validate(ui).expect_err(&format!("se esperaba inválido: {ui}"));
        assert!(
            err.message.contains(needle),
            "el mensaje debe contener {needle:?}: {err}"
        );
    }

    #[test]
    fn un_ui_vacio_es_valido() {
        assert!(validate(&json!({})).is_ok());
    }

    #[test]
    fn un_ui_completo_es_valido() {
        let ui = json!({
            "title": "Configuración del bloque",
            "order": ["url", "headers", "total"],
            "widget": { "url": "text", "headers": "json", "total": "number" },
            "help": { "url": "Admite interpolación {$.auth.userId}", "total": "En centavos" }
        });
        assert!(validate(&ui).is_ok());
    }

    #[test]
    fn el_widget_ref_template_esta_admitido() {
        let ui = json!({ "widget": { "url": REF_TEMPLATE_WIDGET } });
        assert!(validate(&ui).is_ok());
    }

    #[test]
    fn un_ui_que_no_es_objeto_es_invalido() {
        for ui in [json!([]), json!("texto"), json!(null), json!(3)] {
            assert_invalid(&ui, "objeto JSON");
        }
    }

    #[test]
    fn una_clave_fuera_del_contrato_es_invalida() {
        assert_invalid(&json!({ "colores": ["rojo"] }), "colores");
        assert_invalid(&json!({ "helpText": "x" }), "helpText");
    }

    #[test]
    fn el_title_debe_ser_un_string_no_vacio() {
        assert_invalid(&json!({ "title": 7 }), "string");
        assert_invalid(&json!({ "title": "   " }), "vacío");
    }

    #[test]
    fn order_exige_strings_unicos_y_no_vacios() {
        assert_invalid(&json!({ "order": "url" }), "array");
        assert_invalid(&json!({ "order": [] }), "vacío");
        assert_invalid(&json!({ "order": ["url", 7] }), "string");
        assert_invalid(&json!({ "order": ["url", "url"] }), "repetido");
        assert_invalid(&json!({ "order": [" "] }), "vacía");
    }

    #[test]
    fn widget_exige_ids_kebab_case() {
        assert_invalid(&json!({ "widget": [] }), "objeto");
        assert_invalid(&json!({ "widget": { "url": 7 } }), "string");
        assert_invalid(&json!({ "widget": { "url": "" } }), "no es válido");
        assert_invalid(&json!({ "widget": { "url": "Ref-Template" } }), "no es válido");
        assert_invalid(&json!({ "widget": { "url": "-text" } }), "no es válido");
    }

    #[test]
    fn help_exige_textos_no_vacios() {
        assert_invalid(&json!({ "help": "x" }), "objeto");
        assert_invalid(&json!({ "help": { "url": 7 } }), "string");
        assert_invalid(&json!({ "help": { "url": "" } }), "vacío");
    }

    #[test]
    fn el_error_reporta_la_ruta_exacta() {
        let err = validate(&json!({ "widget": { "url": "Ref" } })).expect_err("inválido");
        assert_eq!(err.path, "$.widget.url");
        assert_eq!(err.to_string(), format!("ui inválido en $.widget.url: {}", err.message));
    }
}