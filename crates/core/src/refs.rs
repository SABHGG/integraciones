//! Resolución de referencias `$.` sobre el contexto de ejecución.
//!
//! Una referencia es un string que empieza por `$.` y navega un contexto JSON
//! por segmentos separados por puntos: `$.trigger.body.items.0.total`.
//! Raíces reservadas: `$.trigger` (entrada del disparo) y `$.now` (timestamp
//! ISO de inicio); el resto de raíces son salidas nombradas de bloques previos.
//!
//! La gramática tiene dos anclajes:
//! - **mapeo de entrada** (directo): un string que empieza por `$.` se sustituye
//!   por el nodo resuelto, conservando su tipo JSON ([`resolve_input`]).
//! - **interpolación en strings de config**: cada `{$.ref}` se sustituye por el
//!   texto del valor ([`interpolate`] / [`resolve_config`]).
//!
//! Una referencia que no resuelve es un fallo duro (fail-fast) con un mensaje
//! quirúrgico: la ruta exacta y qué falta para resolverla.

use serde_json::Map;
use serde_json::Value;

/// Un segmento de una ruta de puntos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Clave de objeto (`userId`, `total`, …).
    Key(String),
    /// Índice numérico sobre un array (`0`, `12`, …); también se acepta como
    /// clave numérica de un objeto.
    Index(usize),
}

/// Ruta `$.` ya validada sintácticamente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefPath {
    raw: String,
    segments: Vec<Segment>,
}

/// Error de referencia: sintaxis inválida o ruta no resuelta.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RefError {
    /// La ruta no tiene la forma `$.seg.seg...`.
    #[error("referencia inválida \"{raw}\": {reason}")]
    Syntax {
        /// Texto original de la referencia (o del template, si falló dentro).
        raw: String,
        /// Motivo concreto del rechazo.
        reason: String,
    },
    /// La ruta es válida pero el contexto no la satisface.
    #[error("referencia {raw} no resolvió: {detail}")]
    Unresolved {
        /// Texto original de la referencia.
        raw: String,
        /// Qué falta: raíz ausente, campo ausente, índice fuera de rango, tipo.
        detail: String,
    },
}

impl RefPath {
    /// Parsea una referencia `$.a.b.0`.
    pub fn parse(raw: &str) -> Result<Self, RefError> {
        let syntax = |reason: String| RefError::Syntax {
            raw: raw.to_string(),
            reason,
        };

        let rest = raw
            .strip_prefix(ROOT)
            .ok_or_else(|| syntax("debe empezar por \"$.\"".to_string()))?;
        if rest.is_empty() {
            return Err(syntax("no tiene ningún segmento después de \"$.\"".into()));
        }

        let mut segments = Vec::new();
        for part in rest.split('.') {
            if part.is_empty() {
                return Err(syntax(format!(
                    "tiene un segmento vacío en \"{raw}\" (no se admiten puntos consecutivos ni finales)"
                )));
            }
            if !part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Err(syntax(format!(
                    "el segmento \"{part}\" tiene caracteres no permitidos (solo alfanuméricos, \"_\" y \"-\")"
                )));
            }
            if part.chars().all(|c| c.is_ascii_digit()) {
                let index = part.parse::<usize>().map_err(|_| {
                    syntax(format!("el índice numérico \"{part}\" está fuera de rango"))
                })?;
                segments.push(Segment::Index(index));
            } else {
                segments.push(Segment::Key(part.to_string()));
            }
        }

        Ok(Self {
            raw: raw.to_string(),
            segments,
        })
    }

    /// Texto original de la referencia.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Segmentos en orden de navegación (sin la raíz `$`).
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }
}

/// Prefijo de toda referencia `$.`.
const ROOT: &str = "$.";
/// Ancla de interpolación dentro de un string de config.
const INTERPOLATION: &str = "{$.";

/// `true` si el valor es un string que empieza por `$.` (por tanto, referencia).
pub fn is_ref(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.starts_with(ROOT))
}

/// Resuelve una ruta ya parseada contra el contexto de ejecución.
pub fn resolve<'a>(context: &'a Value, path: &RefPath) -> Result<&'a Value, RefError> {
    let unresolved = |detail: String| RefError::Unresolved {
        raw: path.raw.clone(),
        detail,
    };

    let Value::Object(root) = context else {
        return Err(unresolved(format!(
            "el contexto de ejecución no es un objeto JSON (se encontró {})",
            describe_type(context)
        )));
    };

    let mut segments = path.segments.iter();
    let Some(first) = segments.next() else {
        return Err(RefError::Syntax {
            raw: path.raw.clone(),
            reason: "no tiene ningún segmento".into(),
        });
    };

    let root_key = match first {
        Segment::Key(key) => key.clone(),
        Segment::Index(index) => index.to_string(),
    };
    let mut current = root.get(&root_key).ok_or_else(|| {
        unresolved(format!(
            "falta la raíz \"{root_key}\" en el contexto (raíces disponibles: {})",
            list_keys(root)
        ))
    })?;
    let mut walked = format!("{ROOT}{root_key}");

    for segment in segments {
        match (current, segment) {
            (Value::Object(map), Segment::Key(key)) => {
                current = map.get(key).ok_or_else(|| {
                    unresolved(format!(
                        "falta el campo \"{key}\" en {walked} (campos disponibles: {})",
                        list_keys(map)
                    ))
                })?;
                walked.push('.');
                walked.push_str(key);
            }
            (Value::Object(map), Segment::Index(index)) => {
                // Un segmento numérico también puede ser una clave de objeto.
                let key = index.to_string();
                current = map.get(&key).ok_or_else(|| {
                    unresolved(format!(
                        "falta el campo \"{key}\" en {walked} (campos disponibles: {})",
                        list_keys(map)
                    ))
                })?;
                walked.push('.');
                walked.push_str(&key);
            }
            (Value::Array(items), Segment::Index(index)) => {
                current = items.get(*index).ok_or_else(|| {
                    unresolved(format!(
                        "el índice {index} está fuera de rango en {walked} (longitud {})",
                        items.len()
                    ))
                })?;
                walked.push('.');
                walked.push_str(&index.to_string());
            }
            (Value::Array(_), Segment::Key(key)) => {
                return Err(unresolved(format!(
                    "{walked} es un array y \"{key}\" no es un índice numérico"
                )));
            }
            (other, segment) => {
                return Err(unresolved(format!(
                    "{walked} es de tipo {} y no permite acceder a {}",
                    describe_type(other),
                    describe_segment(segment)
                )));
            }
        }
    }

    Ok(current)
}

/// Parsea y resuelve una referencia textual.
pub fn resolve_ref<'a>(context: &'a Value, raw: &str) -> Result<&'a Value, RefError> {
    let path = RefPath::parse(raw)?;
    resolve(context, &path)
}

/// Resuelve un mapeo de entrada: los strings que empiezan por `$.` se sustituyen
/// por el nodo resuelto (conservando su tipo JSON) y el resto es literal.
pub fn resolve_input(context: &Value, spec: &Value) -> Result<Value, RefError> {
    match spec {
        Value::String(text) if text.starts_with(ROOT) => {
            Ok(resolve_ref(context, text)?.clone())
        }
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_input(context, item))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut resolved = Map::new();
            for (key, value) in map {
                resolved.insert(key.clone(), resolve_input(context, value)?);
            }
            Ok(Value::Object(resolved))
        }
        other => Ok(other.clone()),
    }
}

/// Un tramo de un string de config: texto literal o una referencia `{$.…}`.
enum TemplatePart<'a> {
    /// Texto que se copia tal cual.
    Literal(&'a str),
    /// Texto de una referencia, sin las llaves.
    Ref(&'a str),
}

/// Divide un string de config en tramos literales y referencias.
///
/// Las llaves cuyo contenido no empieza por `$.` no son interpolaciones y se
/// conservan como literal. Una interpolación sin cerrar es un error de sintaxis.
fn scan_template(template: &str) -> Result<Vec<TemplatePart<'_>>, RefError> {
    let mut parts = Vec::new();
    let mut rest = template;

    while let Some(open) = rest.find('{') {
        if open > 0 {
            parts.push(TemplatePart::Literal(&rest[..open]));
        }
        let after_open = &rest[open + 1..];

        // Llave literal (JSON, por ejemplo): no es una interpolación.
        if !after_open.starts_with(ROOT) {
            parts.push(TemplatePart::Literal("{"));
            rest = after_open;
            continue;
        }

        let Some(close) = after_open.find('}') else {
            return Err(RefError::Syntax {
                raw: template.to_string(),
                reason: "interpolación sin cerrar: falta \"}\" después de \"{\"".to_string(),
            });
        };

        parts.push(TemplatePart::Ref(&after_open[..close]));
        rest = &after_open[close + 1..];
    }

    if !rest.is_empty() {
        parts.push(TemplatePart::Literal(rest));
    }
    Ok(parts)
}

/// Interpola todos los `{$.ref}` de un string de config.
///
/// Las llaves cuyo contenido no empieza por `$.` se dejan intactas (permite JSON
/// literal en la config). Los valores no-string se serializan como JSON compacto.
pub fn interpolate(context: &Value, template: &str) -> Result<String, RefError> {
    let mut out = String::with_capacity(template.len());
    for part in scan_template(template)? {
        match part {
            TemplatePart::Literal(text) => out.push_str(text),
            TemplatePart::Ref(raw) => {
                out.push_str(&interpolated_text(resolve_ref(context, raw)?));
            }
        }
    }
    Ok(out)
}

/// Extrae las referencias `{$.…}` de un string de config **sin resolverlas**.
///
/// Es la mitad estática de [`interpolate`]: sirve para validar en tiempo de
/// diseño las refs de una config (publicación de puentes) sin contexto de
/// ejecución. Las llaves que no son refs se ignoran y una interpolación sin
/// cerrar sigue siendo un [`RefError::Syntax`].
pub fn template_refs(template: &str) -> Result<Vec<&str>, RefError> {
    Ok(scan_template(template)?
        .into_iter()
        .filter_map(|part| match part {
            TemplatePart::Ref(raw) => Some(raw),
            TemplatePart::Literal(_) => None,
        })
        .collect())
}

/// Resuelve una config de instancia de forma recursiva: interpola los strings
/// con `{$.ref}`, sustituye los strings que son una referencia completa y
/// respeta el resto de valores como literales.
pub fn resolve_config(context: &Value, spec: &Value) -> Result<Value, RefError> {
    match spec {
        Value::String(text) if text.contains(INTERPOLATION) => {
            Ok(Value::String(interpolate(context, text)?))
        }
        Value::String(text) if text.starts_with(ROOT) => Ok(resolve_ref(context, text)?.clone()),
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_config(context, item))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut resolved = Map::new();
            for (key, value) in map {
                resolved.insert(key.clone(), resolve_config(context, value)?);
            }
            Ok(Value::Object(resolved))
        }
        other => Ok(other.clone()),
    }
}

/// Texto de un valor dentro de una interpolación.
fn interpolated_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
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

/// Segmento legible, para los mensajes de error.
fn describe_segment(segment: &Segment) -> String {
    match segment {
        Segment::Key(key) => format!("\"{key}\""),
        Segment::Index(index) => format!("el índice {index}"),
    }
}

/// Enumera las claves de un objeto en orden alfabético para el mensaje de error.
fn list_keys(map: &Map<String, Value>) -> String {
    if map.is_empty() {
        return "ninguna".to_string();
    }
    let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
    keys.sort_unstable();
    keys.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context() -> Value {
        json!({
            "trigger": {
                "body": {
                    "userId": "u-1",
                    "items": [{ "total": 10 }, { "total": 20 }],
                    "0": "clave-cero"
                },
                "query": { "q": "buscar" }
            },
            "now": "2025-01-02T03:04:05Z",
            "fetchUser": { "id": "u-1", "token": "secreto" }
        })
    }

    // ── 3.1 sintaxis ───────────────────────────────────────────────────

    #[test]
    fn parsea_segmentos_alfanumericos_y_numericos() {
        let path = RefPath::parse("$.trigger.body.items.0.total").expect("ruta válida");
        assert_eq!(path.raw(), "$.trigger.body.items.0.total");
        assert_eq!(
            path.segments(),
            &[
                Segment::Key("trigger".into()),
                Segment::Key("body".into()),
                Segment::Key("items".into()),
                Segment::Index(0),
                Segment::Key("total".into()),
            ]
        );
    }

    #[test]
    fn acepta_guion_y_guion_bajo_en_segmentos() {
        let path = RefPath::parse("$.mi-bloque.mi_campo").expect("ruta válida");
        assert_eq!(
            path.segments(),
            &[
                Segment::Key("mi-bloque".into()),
                Segment::Key("mi_campo".into())
            ]
        );
    }

    #[test]
    fn rechaza_rutas_sin_prefijo_raiz() {
        let err = RefPath::parse("trigger.body").expect_err("debe rechazarse");
        match err {
            RefError::Syntax { raw, reason } => {
                assert_eq!(raw, "trigger.body");
                assert!(reason.contains("$."), "motivo: {reason}");
            }
            other => panic!("se esperaba Syntax, llegó {other:?}"),
        }
    }

    #[test]
    fn rechaza_rutas_malformadas_con_mensaje_quirurgico() {
        for raw in ["$.", "$..body", "$.a..b", "$.a b", "$.a[b]", "$.a.", "$.a-b.c d"] {
            let err = RefPath::parse(raw).expect_err(raw);
            match err {
                RefError::Syntax { raw: got, reason } => {
                    assert_eq!(got, raw);
                    assert!(!reason.is_empty());
                }
                other => panic!("{raw}: se esperaba Syntax, llegó {other:?}"),
            }
        }
    }

    #[test]
    fn distingue_referencia_de_literal() {
        assert!(is_ref(&json!("$.trigger.body")));
        assert!(is_ref(&json!("$.now")));
        assert!(!is_ref(&json!("https://api.x.com/{$.trigger.body.id}")));
        assert!(!is_ref(&json!("$trigger")));
        assert!(!is_ref(&json!(42)));
        assert!(!is_ref(&json!(null)));
    }

    // ── 3.1 resolución ─────────────────────────────────────────────────

    #[test]
    fn resuelve_rutas_anidadas_y_refs_a_nodo_completo() {
        let ctx = context();
        assert_eq!(
            resolve_ref(&ctx, "$.trigger.body.userId").unwrap(),
            &json!("u-1")
        );
        assert_eq!(
            resolve_ref(&ctx, "$.trigger.body.items.1.total").unwrap(),
            &json!(20)
        );
        assert_eq!(
            resolve_ref(&ctx, "$.trigger.body").unwrap(),
            &json!({ "userId": "u-1", "items": [{ "total": 10 }, { "total": 20 }], "0": "clave-cero" })
        );
        assert_eq!(
            resolve_ref(&ctx, "$.now").unwrap(),
            &json!("2025-01-02T03:04:05Z")
        );
        assert_eq!(
            resolve_ref(&ctx, "$.fetchUser").unwrap(),
            &json!({ "id": "u-1", "token": "secreto" })
        );
    }

    #[test]
    fn acepta_indice_numerico_como_clave_de_objeto() {
        let ctx = context();
        assert_eq!(
            resolve_ref(&ctx, "$.trigger.body.0").unwrap(),
            &json!("clave-cero")
        );
    }

    #[test]
    fn raiz_ausente_reporta_las_raices_disponibles() {
        let ctx = context();
        let err = resolve_ref(&ctx, "$.auth.userId").expect_err("no existe la raíz");
        let RefError::Unresolved { raw, detail } = err else {
            panic!("se esperaba Unresolved");
        };
        assert_eq!(raw, "$.auth.userId");
        assert!(detail.contains("\"auth\""), "detalle: {detail}");
        assert!(detail.contains("trigger"), "detalle: {detail}");
        assert!(detail.contains("now"), "detalle: {detail}");
    }

    #[test]
    fn campo_ausente_reporta_la_ruta_exacta_y_los_campos_disponibles() {
        let ctx = context();
        let err = resolve_ref(&ctx, "$.trigger.body.userIdd").expect_err("campo inexistente");
        let RefError::Unresolved { detail, .. } = err else {
            panic!("se esperaba Unresolved");
        };
        assert!(detail.contains("\"userIdd\""), "detalle: {detail}");
        assert!(detail.contains("$.trigger.body"), "detalle: {detail}");
        assert!(detail.contains("userId"), "detalle: {detail}");
    }

    #[test]
    fn indice_fuera_de_rango_reporta_la_longitud() {
        let ctx = context();
        let err = resolve_ref(&ctx, "$.trigger.body.items.9").expect_err("índice fuera");
        let RefError::Unresolved { detail, .. } = err else {
            panic!("se esperaba Unresolved");
        };
        assert!(detail.contains('9'), "detalle: {detail}");
        assert!(detail.contains("$.trigger.body.items"), "detalle: {detail}");
        assert!(detail.contains('2'), "detalle: {detail}");
    }

    #[test]
    fn navegar_dentro_de_un_escalar_es_un_error_sin_panico() {
        let ctx = context();
        for raw in ["$.trigger.body.userId.length", "$.now.x", "$.trigger.body.items.userId"] {
            let err = resolve_ref(&ctx, raw).expect_err(raw);
            assert!(matches!(err, RefError::Unresolved { .. }), "{raw}: {err:?}");
        }
    }

    // ── 3.1 mapeo de entrada ───────────────────────────────────────────

    #[test]
    fn resuelve_mapeo_de_entrada_con_refs_literales_y_anidados() {
        let ctx = context();
        let spec = json!({
            "id": "$.trigger.body.userId",
            "user": "$.fetchUser",
            "literal": "texto normal",
            "numero": 7,
            "bandera": true,
            "nulo": null,
            "anidado": { "token": "$.fetchUser.token", "nota": "fija" },
            "lista": ["$.now", {"total": "$.trigger.body.items.0.total"}]
        });
        let resolved = resolve_input(&ctx, &spec).expect("mapeo válido");
        assert_eq!(
            resolved,
            json!({
                "id": "u-1",
                "user": { "id": "u-1", "token": "secreto" },
                "literal": "texto normal",
                "numero": 7,
                "bandera": true,
                "nulo": null,
                "anidado": { "token": "secreto", "nota": "fija" },
                "lista": ["2025-01-02T03:04:05Z", { "total": 10 }]
            })
        );
    }

    #[test]
    fn mapeo_de_entrada_falla_en_la_primera_ref_no_resuelta() {
        let ctx = context();
        let err = resolve_input(&ctx, &json!({ "id": "$.noExiste.id" })).expect_err("debe fallar");
        let RefError::Unresolved { raw, .. } = err else {
            panic!("se esperaba Unresolved");
        };
        assert_eq!(raw, "$.noExiste.id");
    }

    // ── 3.1 interpolación ──────────────────────────────────────────────

    #[test]
    fn interpola_una_o_varias_refs_en_un_string() {
        let ctx = context();
        assert_eq!(
            interpolate(&ctx, "https://api.x.com/users/{$.trigger.body.userId}").unwrap(),
            "https://api.x.com/users/u-1"
        );
        assert_eq!(
            interpolate(&ctx, "{$.fetchUser.id}:{$.trigger.body.items.1.total}").unwrap(),
            "u-1:20"
        );
        assert_eq!(interpolate(&ctx, "sin refs").unwrap(), "sin refs");
    }

    #[test]
    fn interpola_valores_no_string_como_json_compacto() {
        let ctx = json!({ "bloque": { "ok": true, "n": 3, "nulo": null } });
        assert_eq!(
            interpolate(&ctx, "ok={$.bloque.ok} n={$.bloque.n} z={$.bloque.nulo}").unwrap(),
            "ok=true n=3 z=null"
        );
    }

    #[test]
    fn template_refs_extrae_las_refs_sin_resolverlas() {
        assert_eq!(
            template_refs("https://api.x.com/users/{$.trigger.body.userId}").unwrap(),
            vec!["$.trigger.body.userId"]
        );
        assert_eq!(
            template_refs("{$.a.id}:{$.b.0.x}").unwrap(),
            vec!["$.a.id", "$.b.0.x"]
        );
        assert!(template_refs("sin refs").unwrap().is_empty());
        assert!(template_refs("").unwrap().is_empty());
    }

    #[test]
    fn template_refs_ignora_las_llaves_que_no_son_refs() {
        assert_eq!(
            template_refs("{\"id\":\"{$.fetchUser.id}\"}").unwrap(),
            vec!["$.fetchUser.id"]
        );
        assert!(template_refs("{no-es-ref}").unwrap().is_empty());
        assert!(template_refs("{}").unwrap().is_empty());
    }

    #[test]
    fn template_refs_reporta_una_interpolacion_sin_cerrar() {
        let err = template_refs("https://x/{$.now").expect_err("sin cerrar");
        let RefError::Syntax { raw, reason } = err else {
            panic!("se esperaba Syntax");
        };
        assert_eq!(raw, "https://x/{$.now");
        assert!(reason.contains('}'), "motivo: {reason}");
    }

    #[test]
    fn interpolacion_respeta_llaves_que_no_son_refs() {
        let ctx = context();
        assert_eq!(
            interpolate(&ctx, "{\"id\":\"{$.fetchUser.id}\"}").unwrap(),
            "{\"id\":\"u-1\"}"
        );
    }

    #[test]
    fn interpolacion_sin_cerrar_falla_con_error_de_sintaxis() {
        let ctx = context();
        let err = interpolate(&ctx, "https://x/{$.now").expect_err("sin cerrar");
        let RefError::Syntax { raw, reason } = err else {
            panic!("se esperaba Syntax");
        };
        assert_eq!(raw, "https://x/{$.now");
        assert!(reason.contains('}'), "motivo: {reason}");
    }

    #[test]
    fn interpolacion_de_ref_no_resuelta_falla_con_la_ruta_exacta() {
        let ctx = context();
        let err = interpolate(&ctx, "https://x/{$.trigger.body.nope}").expect_err("no resuelve");
        let RefError::Unresolved { raw, detail } = err else {
            panic!("se esperaba Unresolved");
        };
        assert_eq!(raw, "$.trigger.body.nope");
        assert!(detail.contains("nope"), "detalle: {detail}");
    }

    #[test]
    fn interpolacion_de_ref_malformada_falla_con_error_de_sintaxis() {
        let ctx = context();
        let err = interpolate(&ctx, "{$.trigger..body}").expect_err("malformada");
        assert!(matches!(err, RefError::Syntax { .. }), "{err:?}");
    }

    // ── 3.1 config ─────────────────────────────────────────────────────

    #[test]
    fn resuelve_config_interpolando_strings_y_conservando_tipos_en_refs_completas() {
        let ctx = context();
        let spec = json!({
            "url": "https://api.x.com/users/{$.trigger.body.userId}",
            "limit": 5,
            "headers": { "Authorization": "Bearer {$.fetchUser.token}" },
            "soloRef": "$.trigger.body.items",
            "flags": [true, "prefijo-{$.fetchUser.id}"]
        });
        let resolved = resolve_config(&ctx, &spec).expect("config válida");
        assert_eq!(
            resolved,
            json!({
                "url": "https://api.x.com/users/u-1",
                "limit": 5,
                "headers": { "Authorization": "Bearer secreto" },
                "soloRef": [{ "total": 10 }, { "total": 20 }],
                "flags": [true, "prefijo-u-1"]
            })
        );
    }

    #[test]
    fn config_con_ref_no_resuelta_falla_fast() {
        let ctx = context();
        let err = resolve_config(&ctx, &json!({ "headers": { "x": "{$.auth.token}" } }))
            .expect_err("debe fallar");
        let RefError::Unresolved { raw, .. } = err else {
            panic!("se esperaba Unresolved");
        };
        assert_eq!(raw, "$.auth.token");
    }
}
