//! Gramática declarativa de condiciones sobre el contexto de ejecución.
//!
//! Las condiciones deciden rutas (`when` por instancia y rutas de `switch`)
//! y se evalúan **en el host** sin ejecutar código del usuario: un objeto con
//! un solo operador, referencias `$.` como primer operando y combinadores
//! recursivos.
//!
//! ```json
//! { "and": [
//!     { "gte": ["$.cliente.pedidos", 10] },
//!     { "eq": ["$.cliente.pais", "ES"] }
//! ]}
//! ```
//!
//! Operadores:
//! - `{ "eq": [ref, valor] }` / `{ "ne": [ref, valor] }`: igualdad JSON entre
//!   tipos coherentes (los números se comparan numéricamente, así `10 ==
//!   10.0`; `null` contra otro valor es igualdad falsa, nunca error; tipos
//!   distintos entre operandos es fallo determinista, no `false`).
//! - `{ "gt": [ref, valor] }` (+ `gte`, `lt`, `lte`): orden numérico entre
//!   números u orden lexicográfico entre strings; otro par es fallo.
//! - `{ "exists": ref }`: la referencia resuelve — `null` resuelve (valor
//!   presente), un campo ausente NO resuelve y es fail-fast, igual que en los
//!   mapeos de entrada. Distingue "valor distinto de null" de "ausente".
//! - `{ "contains": [ref, valor] }`: substring si el contenedor es un string;
//!   pertenencia si es un array (el miembro se compara con igualdad laxa, así
//!   un sub-array NUNCA es miembro: `["vip"]` no es miembro de una lista de
//!   strings); otro contenedor es fallo determinista.
//!
//! Combinadores: `{ "and": [conds…] }` (no vacío), `{ "or": [conds…] }` (no
//! vacío) y `{ "not": cond }` (una sola condición). El segundo operando puede
//! ser otra referencia `$.` o un literal JSON.
//!
//! Fallo determinista, dos sabores:
//! - una referencia que no resuelve es fail-fast
//!   ([`ConditionError::Unresolved`], el ejecutor lo expone como
//!   `REF_UNRESOLVED`), igual que en los mapeos de entrada;
//! - operandos incomparables ([`ConditionError::Type`], p. ej. número vs
//!   string en `gt`): fail-fast también, para que un error de configuración
//!   no decida una ruta en silencio.

use crate::refs::{RefPath, resolve};
use serde_json::Value;

/// Un fallo al validar o evaluar una condición.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConditionError {
    /// La condición no es conforme a la gramática declarativa.
    #[error("condición inválida en \"{raw}\": {reason}")]
    Syntax {
        /// Condición textual (JSON compacto) donde se detectó el rechazo.
        raw: String,
        /// Motivo concreto del rechazo.
        reason: String,
    },
    /// Una referencia de la condición no resolvió contra el contexto.
    #[error("referencia {raw} no resolvió al evaluar la condición: {detail}")]
    Unresolved {
        /// Texto original de la referencia.
        raw: String,
        /// Qué falta: raíz ausente, campo ausente, índice fuera de rango, tipo.
        detail: String,
    },
    /// Operandos incomparables en tiempo de ejecución.
    #[error("operandos incomparables en {raw}: {detail}")]
    Type {
        /// Condición textual (JSON compacto) donde se detectó el rechazo.
        raw: String,
        /// Tipos encontrados y qué se exigía.
        detail: String,
    },
}

/// Operadores de un término (comparan o examinan valores).
const OPERATORS: &[&str] = &["eq", "ne", "gt", "gte", "lt", "lte", "exists", "contains"];

/// Combinadores que anidan otras condiciones.
const COMBINATORS: &[&str] = &["and", "or", "not"];

/// Texto compacto de una condición, para los mensajes de error.
fn raw_of(condition: &Value) -> String {
    condition.to_string()
}

fn syntax(condition: &Value, reason: String) -> ConditionError {
    ConditionError::Syntax {
        raw: raw_of(condition),
        reason,
    }
}

/// Destructura el único par clave/valor del objeto condición.
fn sole_key(condition: &Value) -> Result<(&str, &Value), ConditionError> {
    let Value::Object(map) = condition else {
        return Err(syntax(
            condition,
            "las condiciones son declarativas: debe ser un objeto con un solo operador".into(),
        ));
    };
    if map.len() != 1 {
        return Err(syntax(
            condition,
            format!("las condiciones son declarativas: una sola clave (lleva {})", map.len()),
        ));
    }
    let (key, value) = map.iter().next().expect("mapa no vacío");
    Ok((key.as_str(), value))
}

/// Valida que un operando textual sea una referencia bien formada.
fn ref_of(condition: &Value, value: &Value) -> Result<RefPath, ConditionError> {
    match value {
        Value::String(text) if text.starts_with("$.") => RefPath::parse(text)
            .map_err(|err| syntax(condition, format!("la referencia está malformada: {err}"))),
        other => Err(syntax(
            condition,
            format!("la gramática es declarativa: se esperaba una referencia y llegó {other}"),
        )),
    }
}

/// Parsea el cuerpo `[ref, valor]` de un comparador.
fn pair<'a>(condition: &'a Value, body: &'a Value) -> Result<(&'a Value, &'a Value), ConditionError> {
    let Value::Array(items) = body else {
        return Err(syntax(
            condition,
            "debe ser un array de dos elementos [ref, valor]".into(),
        ));
    };
    if items.len() != 2 {
        return Err(syntax(
            condition,
            format!(
                "debe ser un array de dos elementos [ref, valor] (lleva {})",
                items.len()
            ),
        ));
    }
    Ok((&items[0], &items[1]))
}

/// Valida que un array contenedor sea no vacío (para `and`/`or`).
fn non_empty_list<'a>(condition: &'a Value, body: &'a Value, name: &str) -> Result<&'a Vec<Value>, ConditionError> {
    let Value::Array(items) = body else {
        return Err(syntax(
            condition,
            format!("\"{name}\" debe ser un array de condiciones"),
        ));
    };
    if items.is_empty() {
        return Err(syntax(
            condition,
            format!("\"{name}\" necesita al menos una condición"),
        ));
    }
    Ok(items)
}

/// Resuelve un operando: los strings que empiezan por `$.` son referencias,
/// el resto es literal.
fn operand(context: &Value, value: &Value) -> Result<Value, ConditionError> {
    match value {
        Value::String(text) if text.starts_with("$.") => {
            let path = RefPath::parse(text)
                .map_err(|err| syntax(value, format!("la referencia está malformada: {err}")))?;
            match resolve(context, &path) {
                Ok(node) => Ok(node.clone()),
                Err(crate::refs::RefError::Unresolved { raw, detail }) => {
                    Err(ConditionError::Unresolved { raw, detail })
                }
                Err(other) => Err(syntax(
                    value,
                    format!("la referencia no se pudo resolver: {other}"),
                )),
            }
        }
        other => Ok(other.clone()),
    }
}

/// Igualdad laxa: numérica entre números, `null` contra otro valor es falsa
/// (ni error) y el resto es igualdad JSON profunda.
fn json_eq_lenient(a: &Value, b: &Value) -> bool {
    if a.is_null() || b.is_null() {
        return a.is_null() && b.is_null();
    }
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        return x == y;
    }
    a == b
}

/// Igualdad del operador `eq`: tipos distintos entre operandos no nulos es un
/// fallo determinista; `null` se piensa como ausencia (`null == "x"` false,
/// `null == null` true).
fn json_eq(a: &Value, b: &Value) -> Result<bool, ConditionError> {
    if a.is_null() || b.is_null() {
        return Ok(json_eq_lenient(a, b));
    }
    let comparable = (a.is_number() && b.is_number())
        || (a.is_string() && b.is_string())
        || (a.is_boolean() && b.is_boolean())
        || (a.is_array() && b.is_array())
        || (a.is_object() && b.is_object());
    if !comparable {
        return Err(ConditionError::Type {
            raw: raw_of(&serde_json::json!({"eq": [a.clone(), b.clone()]})),
            detail: format!(
                "los operandos deben ser del mismo tipo (llegó {} y {})",
                describe_type(a),
                describe_type(b)
            ),
        });
    }
    Ok(json_eq_lenient(a, b))
}

/// Orden entre números o entre strings lexicográfico; `None` si el par no
/// tiene orden.
fn order(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        return x.partial_cmp(&y);
    }
    if let (Some(x), Some(y)) = (a.as_str(), b.as_str()) {
        return Some(x.cmp(y));
    }
    None
}

/// Tipo legible de un valor JSON, para los mensajes de error.
fn describe_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "booleano",
        Value::Number(_) => "número",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "objeto",
    }
}

/// Evalúa la condición en el contexto.
pub fn evaluate(context: &Value, condition: &Value) -> Result<bool, ConditionError> {
    let (key, body) = sole_key(condition)?;
    match key {
        "and" => {
            let items = non_empty_list(condition, body, "and")?;
            for cond in items {
                if !evaluate(context, cond)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        "or" => {
            let items = non_empty_list(condition, body, "or")?;
            for cond in items {
                if evaluate(context, cond)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        "not" => Ok(!evaluate(context, body)?),
        "exists" => {
            ref_of(condition, body)?;
            // Presencia: si no resuelve falla fast como en un mapeo; `null`
            // resuelve y es valor presente.
            operand(context, body)?;
            Ok(true)
        }
        op if OPERATORS.contains(&op) => {
            let (first, second) = pair(condition, body)?;
            let av = operand(context, first)?;
            let bv = operand(context, second)?;
            match op {
                "eq" => Ok(json_eq(&av, &bv)?),
                "ne" => Ok(!json_eq(&av, &bv)?),
                "gt" | "gte" | "lt" | "lte" => {
                    let orden = order(&av, &bv).ok_or_else(|| ConditionError::Type {
                        raw: raw_of(condition),
                        detail: format!(
                            "{op} exige dos números o dos strings (llegó {} y {})",
                            describe_type(&av),
                            describe_type(&bv)
                        ),
                    })?;
                    Ok(match op {
                        "gt" => orden.is_gt(),
                        "gte" => orden.is_ge(),
                        "lt" => orden.is_lt(),
                        _ => orden.is_le(),
                    })
                }
                _ => match &av {
                    Value::String(text) => match bv.as_str() {
                        Some(sub) => Ok(text.contains(sub)),
                        None => Err(ConditionError::Type {
                            raw: raw_of(condition),
                            detail: format!(
                                "contains sobre un string exige un miembro string (llegó {})",
                                describe_type(&bv)
                            ),
                        }),
                    },
                    Value::Array(items) => {
                        Ok(items.iter().any(|member| json_eq_lenient(member, &bv)))
                    }
                    _ => Err(ConditionError::Type {
                        raw: raw_of(condition),
                        detail: format!(
                            "contains exige un string o un array (llegó {})",
                            describe_type(&av)
                        ),
                    }),
                },
            }
        }
        other => Err(syntax(
            condition,
            format!(
                "las condiciones son declarativas: el operador \"{other}\" no existe. Operadores: {}; combinadores: {}",
                OPERATORS.join(", "),
                COMBINATORS.join(", ")
            ),
        )),
    }
}

/// Valida estructuralmente la condición contra la gramática declarativa, sin
/// un contexto de ejecución (lo que se usa al publicar: las referencias deben
/// ser sintácticamente correctas; si resuelven lo decide el contexto al
/// ejecutar).
pub fn validate(condition: &Value) -> Result<(), ConditionError> {
    let (key, body) = sole_key(condition)?;
    match key {
        "and" | "or" => {
            let items = non_empty_list(condition, body, key)?;
            for cond in items {
                validate(cond)?;
            }
        }
        "not" => validate(body)?,
        "exists" => {
            ref_of(condition, body)?;
        }
        op if OPERATORS.contains(&op) => {
            let (first, second) = pair(condition, body)?;
            ref_of(condition, first)?;
            // El segundo operando puede ser una referencia o un literal.
            if second.as_str().is_some_and(|text| text.starts_with("$.")) {
                ref_of(condition, second)?;
            }
        }
        other => {
            return Err(syntax(
                condition,
                format!(
                    "las condiciones son declarativas: el operador \"{other}\" no existe. Operadores: {}; combinadores: {}",
                    OPERATORS.join(", "),
                    COMBINATORS.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

/// Recoge las referencias `$.` de la condición, en orden de aparición.
pub fn refs(condition: &Value) -> Result<Vec<String>, ConditionError> {
    let (key, body) = sole_key(condition)?;
    match key {
        "and" | "or" => {
            let items = non_empty_list(condition, body, key)?;
            let mut collected = Vec::new();
            for cond in items {
                collected.extend(refs(cond)?);
            }
            Ok(collected)
        }
        "not" => refs(body),
        "exists" => {
            ref_of(condition, body)?;
            let raw = body.as_str().expect("texto de referencia ya validado").to_string();
            Ok(vec![raw])
        }
        op if OPERATORS.contains(&op) => {
            let (first, second) = pair(condition, body)?;
            let mut collected = vec![ref_of(condition, first)?.raw().to_string()];
            if second.as_str().is_some_and(|text| text.starts_with("$.")) {
                collected.push(ref_of(condition, second)?.raw().to_string());
            }
            Ok(collected)
        }
        other => Err(syntax(
            condition,
            format!("el operador \"{other}\" no pertenece a la gramática declarativa"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context() -> Value {
        json!({
            "trigger": { "body": { "id": "u-1", "items": [{ "total": 10 }, { "total": 20 }] } },
            "now": "2026-05-02T09:00:00Z",
            "fetchUser": { "id": "u-1", "name": "ana", "pais": "ES", "pedidos": 12,
                "tags": ["vip", "beta"], "numeros": [10, 20] },
            "orden": null
        })
    }

    // ── operadores: igualdad ───────────────────────────────────────────

    #[test]
    fn eq_compara_strings_numeros_bools_y_valores_json() {
        let ctx = context();
        for (cond, esperado) in [
            (json!({ "eq": ["$.fetchUser.pais", "ES"] }), true),
            (json!({ "eq": ["$.fetchUser.pais", "FR"] }), false),
            (json!({ "eq": ["$.fetchUser.pedidos", 12] }), true),
            // comparación numérica: 12 == 12.0
            (json!({ "eq": ["$.fetchUser.pedidos", 12.0] }), true),
            (json!({ "eq": ["$.trigger.body.id", "$.fetchUser.id"] }), true),
            (json!({ "eq": ["$.orden", null] }), true),
            (json!({ "ne": ["$.fetchUser.pais", "FR"] }), true),
            (json!({ "ne": ["$.fetchUser.pais", "ES"] }), false),
        ] {
            let resultado = evaluate(&ctx, &cond).unwrap_or_else(|e| panic!("{cond}: {e}"));
            assert_eq!(resultado, esperado, "{cond}");
        }
    }

    // ── operadores: orden ──────────────────────────────────────────────

    #[test]
    fn gt_gte_lt_lte_ordenan_numeros() {
        let ctx = context();
        for (cond, esperado) in [
            (json!({ "gt": ["$.fetchUser.pedidos", 10] }), true),
            (json!({ "gt": ["$.fetchUser.pedidos", 12] }), false),
            (json!({ "gte": ["$.fetchUser.pedidos", 12] }), true),
            (json!({ "lt": ["$.fetchUser.pedidos", 20] }), true),
            (json!({ "lte": ["$.trigger.body.items.1.total", 20] }), true),
            (json!({ "lt": ["$.trigger.body.items.1.total", 20] }), false),
        ] {
            let resultado = evaluate(&ctx, &cond).unwrap_or_else(|e| panic!("{cond}: {e}"));
            assert_eq!(resultado, esperado, "{cond}");
        }
    }

    #[test]
    fn el_orden_lexicografico_compara_strings() {
        let ctx = context();
        assert!(evaluate(&ctx, &json!({ "gte": ["$.now", "2026-01-01"] })).unwrap());
        assert!(evaluate(&ctx, &json!({ "lt": ["$.fetchUser.name", "b"] })).unwrap());
        assert!(!evaluate(&ctx, &json!({ "gt": ["$.fetchUser.name", "z"] })).unwrap());
    }

    #[test]
    fn operandos_incomparables_fallan_de_forma_determinista() {
        let ctx = context();
        let err = evaluate(&ctx, &json!({ "gt": ["$.fetchUser.pedidos", "12"] }))
            .expect_err("número vs string");
        assert!(matches!(err, ConditionError::Type { .. }), "{err:?}");
        assert!(err.to_string().contains("incomparables"), "{err}");

        let err = evaluate(&ctx, &json!({ "eq": ["$.fetchUser.tags", "vip"] }))
            .expect_err("array vs string");
        assert!(matches!(err, ConditionError::Type { .. }), "{err:?}");
    }

    // ── operadores: presencia y pertenencia ────────────────────────────

    #[test]
    fn exists_distingue_presente_de_ausente_y_null_resuelve() {
        let ctx = context();
        assert!(evaluate(&ctx, &json!({ "exists": "$.fetchUser.pais" })).unwrap());
        // `null` es un valor presente: resuelve y existe.
        assert!(evaluate(&ctx, &json!({ "exists": "$.orden" })).unwrap());
        // `null == "x"` es igualdad falsa, nunca error.
        assert!(!evaluate(&ctx, &json!({ "eq": ["$.orden", "x"] })).unwrap());
    }

    #[test]
    fn un_campo_ausente_en_exists_falla_fast_como_en_los_mapeos() {
        let ctx = context();
        let err = evaluate(&ctx, &json!({ "exists": "$.cliente.pais" })).expect_err("campo ausente");
        let ConditionError::Unresolved { raw, detail } = err else {
            panic!("se esperaba Unresolved: {err:?}");
        };
        assert_eq!(raw, "$.cliente.pais");
        assert!(detail.contains("cliente"), "detalle: {detail}");
    }

    #[test]
    fn contains_comprueba_substring_y_pertenencia() {
        let ctx = context();
        assert!(evaluate(&ctx, &json!({ "contains": ["$.fetchUser.name", "an"] })).unwrap());
        assert!(!evaluate(&ctx, &json!({ "contains": ["$.fetchUser.name", "zz"] })).unwrap());
        assert!(evaluate(&ctx, &json!({ "contains": ["$.fetchUser.tags", "vip"] })).unwrap());
        // pertenencia numérica: la lista lleva enteros y el miembro llega 10.0
        assert!(evaluate(&ctx, &json!({ "contains": ["$.fetchUser.numeros", 10.0] })).unwrap());
        // pertenencia laxa: un sub-array NO es miembro de una lista de strings
        assert!(!evaluate(&ctx, &json!({ "contains": ["$.fetchUser.tags", ["vip"]] })).unwrap());
    }

    #[test]
    fn contains_sobre_un_escalar_incomparable_es_type_error() {
        let ctx = context();
        let err = evaluate(&ctx, &json!({ "contains": ["$.fetchUser.pedidos", 3] }))
            .expect_err("escalar");
        assert!(matches!(err, ConditionError::Type { .. }), "{err:?}");
    }

    // ── combinadores ───────────────────────────────────────────────────

    #[test]
    fn and_exige_todas_verdaderas_y_or_alguna() {
        let ctx = context();
        let and = json!({ "and": [
            { "gte": ["$.fetchUser.pedidos", 10] },
            { "eq": ["$.fetchUser.pais", "ES"] }
        ]});
        assert!(evaluate(&ctx, &and).unwrap());

        let and_falso = json!({ "and": [
            { "gte": ["$.fetchUser.pedidos", 10] },
            { "eq": ["$.fetchUser.pais", "FR"] }
        ]});
        assert!(!evaluate(&ctx, &and_falso).unwrap());

        let or = json!({ "or": [
            { "eq": ["$.fetchUser.pais", "FR"] },
            { "lt": ["$.fetchUser.pedidos", 100] }
        ]});
        assert!(evaluate(&ctx, &or).unwrap());

        let or_con_ninguna = json!({ "or": [
            { "eq": ["$.fetchUser.pais", "ZZ"] },
            { "eq": ["$.fetchUser.name", "no-existe"] }
        ]});
        assert!(!evaluate(&ctx, &or_con_ninguna).unwrap());
    }

    #[test]
    fn los_combinadores_anidan_recursivamente() {
        let ctx = context();
        let cond = json!({ "not": { "or": [
            { "eq": ["$.fetchUser.pais", "FR"] },
            { "not": { "contains": ["$.fetchUser.tags", "vip"] } }
        ]}});
        assert!(evaluate(&ctx, &cond).unwrap());
    }

    // ── fail-fast por referencia no resuelta (REF_UNRESOLVED) ──────────

    #[test]
    fn una_referencia_no_resuelta_falla_fast_con_la_ruta_exacta() {
        let ctx = context();
        for cond in [
            json!({ "eq": ["$.cliente.pais", "ES"] }),
            json!({ "gt": ["$.cliente.pedidos", 10] }),
            json!({ "exists": "$.cliente.pais" }),
            // también como segundo operando: `$.noExiste` ES una referencia
            json!({ "eq": ["$.fetchUser.pais", "$.noExiste"] }),
        ] {
            let err = evaluate(&ctx, &cond).expect_err("debe fallar con Unresolved");
            let ConditionError::Unresolved { raw, .. } = err else {
                panic!("se esperaba Unresolved, llegó {err:?} en {cond}");
            };
            assert!(
                raw.starts_with("$.cliente") || raw.starts_with("$.noExiste"),
                "{raw}"
            );
        }
    }

    // ── validación estructural (al publicar) ───────────────────────────

    #[test]
    fn validate_acepta_la_gramatica_completa() {
        for cond in [
            json!({ "eq": ["$.a.b", 1] }),
            json!({ "ne": ["$.a.b", "$.otro.c"] }),
            json!({ "gt": ["$.a", 0] }),
            json!({ "gte": ["$.a", 0] }),
            json!({ "lt": ["$.a", 9] }),
            json!({ "lte": ["$.a", 9] }),
            json!({ "exists": "$.a.b" }),
            json!({ "contains": ["$.a.lista", "x"] }),
            json!({ "and": [{ "exists": "$.a" }, { "ne": ["$.a", 1] }] }),
            json!({ "or": [{ "exists": "$.a" }] }),
            json!({ "not": { "eq": ["$.a", 1] } }),
            json!({ "and": [{ "or": [{ "not": { "exists": "$.a.b" } }] }] }),
        ] {
            validate(&cond).unwrap_or_else(|e| panic!("{cond}: {e}"));
        }
    }

    #[test]
    fn validate_rechaza_lo_que_no_es_declarativo() {
        // JS arbitrario, operadores desconocidos y estructura rota.
        for cond in [
            json!("$.x == 3"),
            json!({ "js": "function(val) { return val > 10 }" }),
            json!({ "==": ["$.x", 3] }),
            json!({ "unknown_op": ["$.x", 3] }),
            json!({}),
            json!({ "eq": ["$.x", 3], "gt": ["$.x", 1] }),
            json!({ "eq": ["$.x"] }),
            json!({ "eq": ["$.x", 1, 2] }),
            json!({ "gt": [3, "$.x"] }),
            json!({ "exists": "no-es-una-ref" }),
            json!({ "and": [] }),
            json!({ "or": "una-sola" }),
            json!({ "not": [] }),
        ] {
            let err = validate(&cond).expect_err(cond.to_string().as_str());
            match err {
                ConditionError::Syntax { reason, .. } => {
                    assert!(
                        reason.contains("declarativas") || reason.contains("declarativa")
                            || reason.contains("referencia") || reason.contains("operador")
                            || reason.contains("condiciones") || reason.contains("clave")
                            || reason.contains("elemento") || reason.contains("array")
                            || reason.contains("llegó") || reason.contains("necesita")
                            || reason.contains("lleva") || reason.contains("malformada")
                            || reason.contains("debe"),
                        "motivo poco claro: {reason}"
                    );
                }
                other => panic!("{cond}: se esperaba Syntax, llegó {other:?}"),
            }
        }
    }

    #[test]
    fn validate_rechaza_referencias_malformadas() {
        for cond in [
            json!({ "eq": ["$.a..b", 1] }),
            json!({ "contains": ["$.a[b]", 1] }),
            json!({ "eq": ["$.a", "$.b."] }),
        ] {
            assert!(validate(&cond).is_err(), "{cond}");
        }
    }

    #[test]
    fn refs_recoge_las_referencias_de_la_condicion_en_orden() {
        let cond = json!({ "and": [
            { "gte": ["$.a.x", "$.b.y"] },
            { "not": { "or": [{ "contains": ["$.c", "z"] }] } }
        ]});
        assert_eq!(refs(&cond).unwrap(), vec!["$.a.x", "$.b.y", "$.c"]);
        let err = refs(&json!({ "unknown": ["$.x", 1] })).expect_err("clave desconocida");
        assert!(matches!(err, ConditionError::Syntax { .. }));
    }
}
