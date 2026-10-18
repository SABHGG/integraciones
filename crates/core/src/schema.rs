//! Validador del subconjunto estricto de JSON Schema usado por la plataforma.
//!
//! Decisiones de diseño (design.md): el validador es propio (sin el crate
//! `jsonschema`) para garantizar determinismo entre la validación de entrada,
//! la publicación de bloques y la futura generación de formularios.
//!
//! Keywords soportadas: `type`, `properties`, `required`, `enum`, `items`,
//! `format`. Se ignoran las anotaciones `$schema`, `title`, `description`, `ui`.
//! Cualquier otra keyword es un schema **no soportado** y se rechaza con un
//! error explícito: el subconjunto es cerrado a propósito, no se valida a medias.

use serde_json::Value;

/// Una violación concreta de una instancia contra su schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Ruta de la instancia donde falla (`$`, `$.user.name`, `$.items.0`).
    pub path: String,
    /// Qué se esperaba y qué se encontró.
    pub message: String,
}

/// Error de validación: o el schema no está soportado/malformado, o la
/// instancia no cumple el schema. Nunca hay pánico.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaError {
    /// El schema usa una keyword no soportada o está malformado.
    #[error("schema no soportado en {path}: {reason}")]
    Unsupported {
        /// Ruta del schema donde se detectó el problema.
        path: String,
        /// Motivo concreto (keyword, tipo esperado del schema, …).
        reason: String,
    },
    /// La instancia no cumple el schema.
    #[error("instancia inválida ({} violación/es): {}", .errors.len(), summarize(.errors))]
    Invalid {
        /// Todas las violaciones detectadas, en orden de recorrido.
        errors: Vec<ValidationError>,
    },
}

/// Keywords de anotación que no afectan a la validación (metadatos de UI y
/// documentación) y por tanto se ignoran.
const ANNOTATIONS: [&str; 4] = ["$schema", "title", "description", "ui"];

/// Formatos soportados por el subconjunto estricto.
const SUPPORTED_FORMATS: [&str; 8] = [
    "date-time",
    "date",
    "email",
    "uri",
    "uuid",
    "ipv4",
    "ipv6",
    "hostname",
];

/// Tipos del subconjunto estricto.
const SUPPORTED_TYPES: [&str; 7] = [
    "object", "array", "string", "number", "integer", "boolean", "null",
];

/// Valida `instance` contra `schema`.
///
/// Primero verifica que el schema pertenece al subconjunto soportado (falla con
/// [`SchemaError::Unsupported`] si usa cualquier keyword fuera del mismo) y
/// después valida la instancia, acumulando todas las violaciones.
pub fn validate(schema: &Value, instance: &Value) -> Result<(), SchemaError> {
    check_schema(schema, "$")?;

    let mut errors = Vec::new();
    check_instance(schema, "$", instance, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(SchemaError::Invalid { errors })
    }
}

/// Verifica que un documento JSON Schema pertenece al subconjunto estricto.
///
/// Es la puerta pública de [`check_schema`] para quien solo tiene el documento
/// (la publicación de bloques valida `configSchema`/`inputSchema`/
/// `outputSchema` sin instancia) y para la futura generación de formularios:
/// a diferencia de [`validate`] no necesita una instancia, y a diferencia de
/// `check_schema` forma parte de la API pública del crate.
pub fn validate_schema_doc(schema: &Value) -> Result<(), SchemaError> {
    check_schema(schema, "$")
}

/// Recorre el schema comprobando que solo usa keywords soportadas y bien
/// formadas. Es independiente de la instancia: un schema no soportado se
/// detecta siempre, aunque la instancia nunca llegase a tocarlo.
fn check_schema(schema: &Value, path: &str) -> Result<(), SchemaError> {
    let Value::Object(map) = schema else {
        return Err(unsupported(
            path,
            format!(
                "un schema debe ser un objeto JSON (se encontró {})",
                describe_value(schema)
            ),
        ));
    };

    for (key, value) in map {
        match key.as_str() {
            k if ANNOTATIONS.contains(&k) => {}
            "type" => check_type_keyword(value, path)?,
            "properties" => {
                let Value::Object(properties) = value else {
                    return Err(unsupported(
                        path,
                        "\"properties\" debe ser un objeto de schemas".to_string(),
                    ));
                };
                for (name, sub) in properties {
                    check_schema(sub, &format!("{path}.properties.{name}"))?;
                }
            }
            "required" => {
                let Value::Array(names) = value else {
                    return Err(unsupported(
                        path,
                        "\"required\" debe ser un array de strings".to_string(),
                    ));
                };
                if names.is_empty() || !names.iter().all(Value::is_string) {
                    return Err(unsupported(
                        path,
                        "\"required\" debe ser un array no vacío de strings".to_string(),
                    ));
                }
            }
            "enum" => {
                let Value::Array(values) = value else {
                    return Err(unsupported(
                        path,
                        "\"enum\" debe ser un array de valores".to_string(),
                    ));
                };
                if values.is_empty() {
                    return Err(unsupported(
                        path,
                        "\"enum\" no puede ser un array vacío".to_string(),
                    ));
                }
            }
            "items" => {
                if value.is_array() {
                    return Err(unsupported(
                        path,
                        "\"items\" con lista de schemas (forma tupla) no está soportado".to_string(),
                    ));
                }
                check_schema(value, &format!("{path}.items"))?;
            }
            "format" => {
                let Some(name) = value.as_str() else {
                    return Err(unsupported(
                        path,
                        "\"format\" debe ser un string".to_string(),
                    ));
                };
                if !SUPPORTED_FORMATS.contains(&name) {
                    return Err(unsupported(
                        path,
                        format!(
                            "el formato \"{name}\" no está soportado (admitidos: {})",
                            SUPPORTED_FORMATS.join(", ")
                        ),
                    ));
                }
            }
            other => {
                return Err(unsupported(
                    path,
                    format!("la keyword \"{other}\" no pertenece al subconjunto soportado"),
                ));
            }
        }
    }

    Ok(())
}

/// Comprueba el `type` (string o lista de strings) del subconjunto.
fn check_type_keyword(value: &Value, path: &str) -> Result<(), SchemaError> {
    let names: Vec<&str> = match value {
        Value::String(name) => vec![name.as_str()],
        Value::Array(items) => {
            let mut names = Vec::with_capacity(items.len());
            for item in items {
                let Some(name) = item.as_str() else {
                    return Err(unsupported(
                        path,
                        "\"type\" debe ser un string o un array de strings".to_string(),
                    ));
                };
                names.push(name);
            }
            names
        }
        other => {
            return Err(unsupported(
                path,
                format!(
                    "\"type\" debe ser un string o un array de strings (se encontró {})",
                    describe_value(other)
                ),
            ));
        }
    };

    if names.is_empty() {
        return Err(unsupported(
            path,
            "\"type\" no puede ser una lista vacía".to_string(),
        ));
    }
    for name in names {
        if !SUPPORTED_TYPES.contains(&name) {
            return Err(unsupported(
                path,
                format!(
                    "el tipo \"{name}\" no está soportado (admitidos: {})",
                    SUPPORTED_TYPES.join(", ")
                ),
            ));
        }
    }

    Ok(())
}

/// Valida una instancia (o subinstancia) contra un schema ya verificado.
fn check_instance(schema: &Value, path: &str, instance: &Value, errors: &mut Vec<ValidationError>) {
    let Value::Object(map) = schema else {
        return; // imposible: `check_schema` ya rechazó los schemas no-objeto
    };

    if let Some(expected) = map.get("type") {
        let satisfied = match expected {
            Value::String(name) => matches_type(name, instance),
            Value::Array(names) => names
                .iter()
                .filter_map(Value::as_str)
                .any(|name| matches_type(name, instance)),
            _ => true,
        };
        if !satisfied {
            errors.push(ValidationError {
                path: path.to_string(),
                message: format!(
                    "se esperaba {} y se encontró {}",
                    describe_expected(expected),
                    describe_value(instance)
                ),
            });
        }
    }

    if let Some(Value::Array(allowed)) = map.get("enum")
        && !allowed.iter().any(|value| value == instance)
    {
        errors.push(ValidationError {
            path: path.to_string(),
            message: format!(
                "el valor no está en el enum permitido ({})",
                allowed
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }

    match instance {
        Value::Object(object) => {
            if let Some(Value::Object(properties)) = map.get("properties") {
                for (name, sub) in properties {
                    if let Some(value) = object.get(name) {
                        check_instance(sub, &format!("{path}.{name}"), value, errors);
                    }
                }
            }
            if let Some(Value::Array(required)) = map.get("required") {
                for name in required.iter().filter_map(Value::as_str) {
                    if !object.contains_key(name) {
                        errors.push(ValidationError {
                            path: path.to_string(),
                            message: format!("falta el campo obligatorio \"{name}\""),
                        });
                    }
                }
            }
        }
        Value::Array(items) => {
            if let Some(sub) = map.get("items") {
                for (index, item) in items.iter().enumerate() {
                    check_instance(sub, &format!("{path}.{index}"), item, errors);
                }
            }
        }
        _ => {}
    }

    if let Some(Value::String(format)) = map.get("format")
        && let Value::String(text) = instance
        && !matches_format(format, text)
    {
        errors.push(ValidationError {
            path: path.to_string(),
            message: format!("el valor no cumple el formato \"{format}\""),
        });
    }
}

/// `true` si la instancia satisface un tipo del subconjunto.
fn matches_type(name: &str, instance: &Value) -> bool {
    match name {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "number" => instance.is_number(),
        "integer" => match instance {
            Value::Number(number) => {
                number.is_i64() || number.is_u64() || number.as_f64().is_some_and(|f| f.fract() == 0.0)
            }
            _ => false,
        },
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        _ => false,
    }
}

/// `true` si el texto cumple el formato soportado.
fn matches_format(format: &str, text: &str) -> bool {
    match format {
        "date-time" => chrono::DateTime::parse_from_rfc3339(text).is_ok(),
        "date" => is_date(text),
        "email" => is_email(text),
        "uri" => is_uri(text),
        "uuid" => uuid::Uuid::parse_str(text).is_ok(),
        "ipv4" => text.parse::<std::net::Ipv4Addr>().is_ok(),
        "ipv6" => text.parse::<std::net::Ipv6Addr>().is_ok(),
        "hostname" => is_hostname(text),
        _ => false,
    }
}

/// Fecha `YYYY-MM-DD` estricta (chrono por sí solo acepta dígitos sin rellenar).
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
        && chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
}

/// Email mínimo razonable: `local@dominio.con.puntos`, sin espacios.
fn is_email(text: &str) -> bool {
    if text.len() > 254 || text.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((local, domain)) = text.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !local.contains('@')
        && !domain.is_empty()
        && domain.contains('.')
        && domain.split('.').all(is_dns_label)
}

/// URI con esquema (`https://…`, `mailto:…`), sin espacios.
fn is_uri(text: &str) -> bool {
    if text.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((scheme, rest)) = text.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && !rest.is_empty()
}

/// Hostname sin espacios: etiquetas DNS válidas.
fn is_hostname(text: &str) -> bool {
    !text.is_empty() && text.len() <= 253 && text.split('.').all(is_dns_label)
}

/// Etiqueta DNS: alfanumérica y `-`, sin empezar ni terminar por `-`.
fn is_dns_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

/// Constructor de un [`SchemaError::Unsupported`].
fn unsupported(path: &str, reason: String) -> SchemaError {
    SchemaError::Unsupported {
        path: path.to_string(),
        reason,
    }
}

/// Describe el tipo JSON de un valor, para los mensajes de error.
fn describe_value(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Describe lo que declara la keyword `type`.
fn describe_expected(expected: &Value) -> String {
    match expected {
        Value::String(name) => format!("type {name}"),
        Value::Array(names) => format!(
            "uno de los tipos [{}]",
            names
                .iter()
                .map(|name| name.as_str().unwrap_or("?").to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("type {}", other),
    }
}

/// Formatea las violaciones en un texto corto y estable para logs y errores.
fn summarize(errors: &[ValidationError]) -> String {
    errors
        .iter()
        .map(|e| format!("{}: {}", e.path, e.message))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn errors(schema: &Value, instance: &Value) -> Vec<ValidationError> {
        match validate(schema, instance) {
            Ok(()) => Vec::new(),
            Err(SchemaError::Invalid { errors }) => errors,
            Err(other) => panic!("se esperaba Invalid, llegó {other:?}"),
        }
    }

    fn assert_invalid(schema: &Value, instance: &Value) {
        let errs = errors(schema, instance);
        assert!(!errs.is_empty(), "se esperaba inválido: {instance}");
    }

    fn assert_unsupported(schema: &Value, needle: &str) {
        match validate(schema, &json!(1)) {
            Err(SchemaError::Unsupported { reason, .. }) => {
                assert!(reason.contains(needle), "motivo: {reason}");
            }
            other => panic!("se esperaba Unsupported({needle}), llegó {other:?}"),
        }
    }

    // ── type ───────────────────────────────────────────────────────────

    #[test]
    fn schema_vacio_acepta_cualquier_valor() {
        for instance in [json!(1), json!("x"), json!(null), json!([1]), json!({"a": 1})] {
            assert!(validate(&json!({}), &instance).is_ok(), "{instance}");
        }
    }

    #[test]
    fn valida_cada_tipo_basico() {
        let cases = [
            ("object", json!({"a": 1}), json!([1])),
            ("array", json!([1]), json!({"a": 1})),
            ("string", json!("hola"), json!(1)),
            ("number", json!(1.5), json!("1.5")),
            ("integer", json!(3), json!(1.5)),
            ("boolean", json!(true), json!(0)),
            ("null", json!(null), json!(false)),
        ];
        for (ty, ok, bad) in cases {
            let schema = json!({ "type": ty });
            assert!(
                validate(&schema, &ok).is_ok(),
                "{ty} debería aceptar {ok}"
            );
            assert_invalid(&schema, &bad);
        }
    }

    #[test]
    fn integer_acepta_flotantes_sin_parte_fraccionaria() {
        assert!(validate(&json!({"type": "integer"}), &json!(2.0)).is_ok());
        assert_invalid(&json!({"type": "integer"}), &json!(2.5));
    }

    #[test]
    fn type_admite_lista_de_tipos() {
        let schema = json!({ "type": ["string", "null"] });
        assert!(validate(&schema, &json!("x")).is_ok());
        assert!(validate(&schema, &json!(null)).is_ok());
        assert_invalid(&schema, &json!(1));
    }

    #[test]
    fn type_malformado_es_error_explicito_sin_panico() {
        assert_unsupported(&json!({ "type": 42 }), "type");
        assert_unsupported(&json!({ "type": "float" }), "float");
        assert_unsupported(&json!({ "type": ["string", 7] }), "type");
    }

    // ── properties / required ──────────────────────────────────────────

    #[test]
    fn valida_properties_y_reporta_la_ruta_del_campo() {
        let schema = json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": { "name": { "type": "string" }, "age": { "type": "integer" } }
                }
            }
        });
        assert!(validate(&schema, &json!({"user": {"name": "ana", "age": 30}})).is_ok());

        let errs = errors(&schema, &json!({"user": {"name": 7, "age": 30}}));
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].path, "$.user.name");
        assert!(errs[0].message.contains("string"), "{errs:?}");
    }

    #[test]
    fn properties_ignora_campos_no_declarados() {
        let schema = json!({ "type": "object", "properties": { "a": { "type": "string" } } });
        assert!(validate(&schema, &json!({"a": "x", "extra": 1})).is_ok());
    }

    #[test]
    fn required_exige_los_campos_y_los_nombra() {
        let schema = json!({
            "type": "object",
            "required": ["id", "nombre"],
            "properties": { "id": { "type": "string" } }
        });
        assert!(validate(&schema, &json!({"id": "1", "nombre": "x"})).is_ok());

        let errs = errors(&schema, &json!({"id": "1"}));
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].message.contains("nombre"), "{errs:?}");
        assert_eq!(errs[0].path, "$");
    }

    #[test]
    fn required_malformado_es_error_explicito() {
        assert_unsupported(&json!({ "required": "id" }), "required");
        assert_unsupported(&json!({ "required": [1] }), "required");
    }

    // ── enum ───────────────────────────────────────────────────────────

    #[test]
    fn enum_acepta_solo_los_valores_listados() {
        let schema = json!({ "enum": ["a", "b", 3, null] });
        assert!(validate(&schema, &json!("a")).is_ok());
        assert!(validate(&schema, &json!(3)).is_ok());
        assert!(validate(&schema, &json!(null)).is_ok());
        assert_invalid(&schema, &json!("c"));
        assert_invalid(&schema, &json!(3.5));
    }

    #[test]
    fn enum_vacio_o_malformado_es_error_explicito() {
        assert_unsupported(&json!({ "enum": [] }), "enum");
        assert_unsupported(&json!({ "enum": "a" }), "enum");
    }

    // ── items ──────────────────────────────────────────────────────────

    #[test]
    fn items_valida_cada_elemento_del_array() {
        let schema = json!({ "type": "array", "items": { "type": "integer" } });
        assert!(validate(&schema, &json!([1, 2, 3])).is_ok());

        let errs = errors(&schema, &json!([1, "dos", 3]));
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].path, "$.1");
    }

    #[test]
    fn items_con_lista_de_schemas_no_esta_soportado() {
        assert_unsupported(
            &json!({ "type": "array", "items": [{ "type": "integer" }] }),
            "items",
        );
    }

    // ── format ─────────────────────────────────────────────────────────

    #[test]
    fn valida_formatos_soportados() {
        let ok = [
            ("date-time", json!("2025-01-02T03:04:05Z")),
            ("date", json!("2025-01-02")),
            ("email", json!("ana@example.com")),
            ("uri", json!("https://example.com/x?y=1")),
            ("uuid", json!("018f3a3e-1c1a-7a1a-9d3e-6f5b8c9d0e1f")),
            ("ipv4", json!("127.0.0.1")),
            ("ipv6", json!("::1")),
            ("hostname", json!("api.example.com")),
        ];
        for (format, value) in ok {
            let schema = json!({ "type": "string", "format": format });
            assert!(validate(&schema, &value).is_ok(), "{format} → {value}");
        }

        let bad = [
            ("date-time", json!("2025-13-02 03:04:05")),
            ("date", json!("2025-1-2")),
            ("email", json!("no-es-email")),
            ("uri", json!("example.com")),
            ("uuid", json!("no-uuid")),
            ("ipv4", json!("999.1.1.1")),
            ("ipv6", json!("no-ip")),
            ("hostname", json!("-mal.com")),
        ];
        for (format, value) in bad {
            let schema = json!({ "type": "string", "format": format });
            assert_invalid(&schema, &value);
        }
    }

    #[test]
    fn format_desconocido_es_error_explicito() {
        assert_unsupported(&json!({ "type": "string", "format": "password" }), "password");
        assert_unsupported(&json!({ "type": "string", "format": 7 }), "format");
    }

    // ── subconjunto cerrado ────────────────────────────────────────────

    #[test]
    fn keywords_fuera_del_subconjunto_se_rechazan_explicitamente() {
        assert_unsupported(&json!({ "oneOf": [{ "type": "string" }] }), "oneOf");
        assert_unsupported(&json!({ "anyOf": [{ "type": "string" }] }), "anyOf");
        assert_unsupported(&json!({ "$ref": "#/defs/x" }), "$ref");
        assert_unsupported(&json!({ "minimum": 1 }), "minimum");
        assert_unsupported(&json!({ "additionalProperties": false }), "additionalProperties");
        assert_unsupported(&json!({ "pattern": "^a" }), "pattern");
    }

    #[test]
    fn anotaciones_de_ui_y_documentacion_no_rompen_la_validacion() {
        let schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "Entrada",
            "description": "datos del bloque",
            "ui": { "order": ["id"], "widget": { "id": "text" } },
            "type": "object",
            "properties": { "id": { "type": "string", "title": "ID", "ui": { "help": "x" } } }
        });
        assert!(validate(&schema, &json!({"id": "a"})).is_ok());
    }

    #[test]
    fn validate_schema_doc_acepta_el_subconjunto_y_rechaza_lo_demas() {
        // Válidos: vacío, completo con anotaciones ignoradas y anidamiento.
        assert!(validate_schema_doc(&json!({})).is_ok());
        assert!(
            validate_schema_doc(&json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "title": "Entrada",
                "type": "object",
                "required": ["id"],
                "properties": {
                    "id": { "type": "string", "format": "uuid", "ui": { "widget": "text" } },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "modo": { "enum": ["a", "b"] }
                }
            }))
            .is_ok()
        );

        // Fuera del subconjunto (keyword, tipo y forma), incluido lo anidado.
        let rechazados = [
            json!({ "pattern": "^a" }),
            json!({ "oneOf": [{ "type": "string" }] }),
            json!({ "type": "float" }),
            json!({ "required": [] }),
            json!({ "type": "array", "items": [{ "type": "integer" }] }),
            json!({ "type": "object", "properties": { "x": { "$ref": "#/defs/x" } } }),
            json!({ "type": "string", "format": "password" }),
        ];
        for schema in rechazados {
            assert!(
                matches!(validate_schema_doc(&schema), Err(SchemaError::Unsupported { .. })),
                "debería rechazarse: {schema}"
            );
        }

        // Un documento que no es objeto también es Unsupported.
        for schema in [json!(true), json!("object"), json!(null)] {
            assert!(matches!(
                validate_schema_doc(&schema),
                Err(SchemaError::Unsupported { .. })
            ));
        }

        // El error nombra la ruta exacta del documento.
        let err = validate_schema_doc(&json!({
            "type": "object",
            "properties": { "x": { "minimum": 1 } }
        }))
        .expect_err("minimum no está soportado");
        assert_eq!(err.to_string(), "schema no soportado en $.properties.x: la keyword \"minimum\" no pertenece al subconjunto soportado");
    }

    #[test]
    fn schema_que_no_es_objeto_es_error_explicito() {
        assert!(matches!(
            validate(&json!("object"), &json!({})),
            Err(SchemaError::Unsupported { .. })
        ));
        assert!(matches!(
            validate(&json!(true), &json!({})),
            Err(SchemaError::Unsupported { .. })
        ));
    }

    // ── recolección de errores ─────────────────────────────────────────

    #[test]
    fn acumula_varias_violaciones_en_una_sola_pasada() {
        let schema = json!({
            "type": "object",
            "required": ["a", "b"],
            "properties": {
                "a": { "type": "string" },
                "c": { "type": "array", "items": { "type": "integer" } }
            }
        });
        let errs = errors(&schema, &json!({ "a": 1, "c": [1, "x"] }));
        assert!(errs.len() >= 3, "{errs:?}");
        let paths: Vec<&str> = errs.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"$.a"), "{paths:?}");
        assert!(paths.contains(&"$.c.1"), "{paths:?}");
    }

    #[test]
    fn el_display_del_error_resume_las_violaciones() {
        let schema = json!({ "type": "object", "required": ["id"] });
        let err = validate(&schema, &json!({})).expect_err("inválido");
        let text = err.to_string();
        assert!(text.contains("id"), "{text}");
        assert!(text.contains('$'), "{text}");
    }
}
