//! API REST de puentes: borrador mutable (`bridges.draft`) + versiones
//! inmutables (`bridge_versions`) con snapshot JSONB completo.
//!
//! El borrador guarda la misma forma que `bridge_versions.definition` pero con
//! instancias SIN resolver: sin `code` copiado y sin schemas propios; los
//! reales viven en las `block_versions` referenciadas por `blockId` +
//! `blockVersionId` de cada instancia. Al publicar se resuelven (copiando
//! `code`/schemas al snapshot), se reejecuta la validación completa y la
//! versión queda inmutable con `UNIQUE(bridge_id, version)`.
//!
//! La validación (usada por `POST /bridges/{id}/validate` y por la publicación)
//! acumula TODOS los errores de una vez: estructura de cadena lineal,
//! compatibilidad de schemas campo a campo y referencias `$.` / `{$.…}`
//! verificables contra los `outputSchema` de los bloques previos.

// Los handlers devuelven `Problem` por valor como todo el crate; el lint de
// tamaño se autoriza a nivel de módulo igual que en blocks.rs.
#![allow(clippy::result_large_err)]

use std::collections::HashMap;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Extension, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use domain::definition::{BlockInstance, BridgeDefinition, TriggerDefinition};
use domain::refs::{RefPath, Segment};
use domain::schema::{PathResolution, resolve_path};

use crate::auth::AuthedKey;
use crate::cursor;
use crate::problem::{self, Problem};
use crate::rest::{self, Page, PageQuery};
use crate::state::AppState;

/// Instancia del borrador ya resuelta contra su `block_versions`.
#[derive(Debug, Clone, PartialEq)]
struct Instance {
    id: String,
    name: String,
    block_id: String,
    block_version_id: String,
    input: Value,
    config: Value,
    input_schema: Value,
    output_schema: Value,
    code: Option<String>,
    /// Condición declarativa opcional (`domain::conditions`).
    when: Option<Value>,
    timeout_ms: Option<u64>,
    /// `false` si los schemas no se pudieron resolver contra la biblioteca.
    resolved: bool,
}

/// Un error de validación del documento.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Issue {
    field: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    block: Option<String>,
}

impl Issue {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            block: None,
        }
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("Issue es serializable")
    }
}

/// Verbos HTTP que acepta un trigger `http` (en mayúsculas, tal como llega por
/// el plano de invocación).
const HTTP_METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

/// Valida la forma del trigger del borrador.
///
/// En el borrador se permite estar incompleto (los faltantes se reportan);
/// como la publicación reusa esta función, un borrador con errores aquí no se
/// puede publicar.
fn validate_trigger(trigger: Option<&Value>) -> Vec<Issue> {
    let Some(trigger) = trigger else {
        return vec![Issue::new("trigger", "el puente necesita un disparador")];
    };
    let Some(kind) = trigger.get("type").and_then(Value::as_str) else {
        return vec![Issue::new(
            "trigger.type",
            "el disparador necesita \"type\" (http|cron|event|manual)",
        )];
    };
    let mut issues = Vec::new();
    if kind == "http" {
        match trigger.get("method").and_then(Value::as_str) {
            None => issues.push(Issue::new(
                "trigger.method",
                "un disparador http necesita \"method\"",
            )),
            Some(method) if !HTTP_METHODS.contains(&method) => issues.push(Issue::new(
                "trigger.method",
                format!(
                    "\"{method}\" no es un verbo HTTP soportado (usa mayúsculas: GET, POST, PUT, PATCH, DELETE)"
                ),
            )),
            Some(_) => {}
        }
        match trigger.get("path").and_then(Value::as_str) {
            None => issues.push(Issue::new(
                "trigger.path",
                "un disparador http necesita \"path\" (la ruta pública del puente)",
            )),
            Some(path) if !path.starts_with('/') => issues.push(Issue::new(
                "trigger.path",
                format!("el path \"{path}\" debe empezar por \"/\""),
            )),
            Some(_) => {}
        }
    } else if kind == "cron" {
        // MVP2: el cron pasa a estar soportado de verdad — schedule y timezone
        // se validan aquí y quedan congelados en el snapshot.
        match trigger.get("schedule").and_then(Value::as_str) {
            None => issues.push(Issue::new(
                "trigger.schedule",
                "un disparador cron necesita \"schedule\" (expresión cron de 5 campos)",
            )),
            Some(schedule) => {
                if let Err(error) = domain::cron::validate(schedule) {
                    issues.push(Issue::new(
                        "trigger.schedule",
                        format!("\"{schedule}\" no es un cron de 5 campos válido: {error}"),
                    ));
                }
            }
        }
        match trigger.get("timezone").and_then(Value::as_str) {
            None => issues.push(Issue::new(
                "trigger.timezone",
                "un disparador cron necesita \"timezone\" (zona IANA, p. ej. Europe/Madrid)",
            )),
            Some(timezone) => {
                if !domain::cron::validate_timezone(timezone) {
                    issues.push(Issue::new(
                        "trigger.timezone",
                        format!("\"{timezone}\" no es una zona horaria IANA conocida"),
                    ));
                }
            }
        }
    } else if kind == "event" {
        // MVP3: el topic de suscripción se valida al publicar y queda
        // congelado en el snapshot.
        match trigger.get("topic").and_then(Value::as_str) {
            None => issues.push(Issue::new(
                "trigger.topic",
                "un disparador event necesita \"topic\" (el canal al que se suscribe el puente)",
            )),
            Some(topic) if !domain::definition::validar_topic(topic) => issues.push(Issue::new(
                "trigger.topic",
                format!(
                    "\"{topic}\" no es un topic válido (minúsculas, dígitos, puntos y guiones; sin separadores al inicio, al final ni consecutivos)"
                ),
            )),
            Some(_) => {}
        }
    } else if kind != "event" && kind != "manual" {
        issues.push(Issue::new(
            "trigger.type",
            format!("\"{kind}\" no es un tipo de disparador soportado (http|cron|event|manual)"),
        ));
    }
    issues
}

/// Valida la cadena lineal: estructura, refs y compatibilidad de schemas.
///
/// Acumula todos los errores de una vez: estructura por instancia, refs de
/// `input`/`config` contra los `outputSchema` de los bloques previos y tipos
/// campo a campo contra el `inputSchema` del destino.
fn validate_chain(instances: &[Instance]) -> Vec<Issue> {
    let mut issues = Vec::new();
    if instances.is_empty() {
        issues.push(Issue::new(
            "blocks",
            "la cadena necesita al menos un bloque",
        ));
        return issues;
    }
    for (index, instance) in instances.iter().enumerate() {
        if instance.id.trim().is_empty() {
            issues.push(Issue::new(
                format!("blocks[{index}].id"),
                "cada instancia necesita un id único dentro de la definición",
            ));
        }
        if instance.name.trim().is_empty() {
            issues.push(Issue::new(
                format!("blocks[{index}].name"),
                "cada instancia necesita un name (bajo él se registra su salida)",
            ));
        }
        if let Some(prev) = instances[..index]
            .iter()
            .position(|other| other.id == instance.id)
        {
            issues.push(Issue::new(
                format!("blocks[{index}].id"),
                format!("el id \"{}\" ya lo usa blocks[{prev}]", instance.id),
            ));
        }
        if let Some(prev) = instances[..index]
            .iter()
            .position(|other| other.name == instance.name)
        {
            issues.push(Issue::new(
                format!("blocks[{index}].name"),
                format!(
                    "el nombre \"{}\" ya lo usa blocks[{prev}]: las salidas se registran por \
                     name y se pisarían",
                    instance.name
                ),
            ));
        }
        if instance.name == "trigger" || instance.name == "now" {
            issues.push(Issue::new(
                format!("blocks[{index}].name"),
                format!(
                    "\"{}\" es un nombre reservado del contexto ($.trigger y $.now)",
                    instance.name
                ),
            ));
        }
        walk_refs(
            instances,
            index,
            "input",
            &instance.input,
            String::new(),
            &mut issues,
        );
        walk_refs(
            instances,
            index,
            "config",
            &instance.config,
            String::new(),
            &mut issues,
        );
    }
    issues
}

/// Campo de un issue: `blocks[1].input.x`, `blocks[2].config.url`, …
fn field(index: usize, source: &str, rel: &str) -> String {
    format!("blocks[{index}].{source}{rel}")
}

/// Recorre recursivamente un valor del mapeo buscando referencias.
///
/// En `input` solo los strings completos `$.…` son refs; en `config` también
/// cuentan las interpolaciones `{$.…}` dentro de un texto.
fn walk_refs(
    instances: &[Instance],
    index: usize,
    source: &str,
    value: &Value,
    rel: String,
    issues: &mut Vec<Issue>,
) {
    match value {
        Value::String(text) => {
            if text.starts_with("$.") {
                check_ref(instances, index, source, &rel, text, issues);
            } else if source == "config" && text.contains('{') {
                match domain::refs::template_refs(text) {
                    Err(error) => {
                        issues.push(Issue::new(field(index, source, &rel), format!("{error}")))
                    }
                    Ok(refs) => {
                        for raw in refs {
                            check_ref(instances, index, source, &rel, raw, issues);
                        }
                    }
                }
            }
        }
        Value::Array(items) => {
            for (position, item) in items.iter().enumerate() {
                walk_refs(
                    instances,
                    index,
                    source,
                    item,
                    format!("{rel}.{position}"),
                    issues,
                );
            }
        }
        Value::Object(map) => {
            for (key, nested) in map {
                walk_refs(
                    instances,
                    index,
                    source,
                    nested,
                    format!("{rel}.{key}"),
                    issues,
                );
            }
        }
        _ => {}
    }
}

/// Valida una referencia `$.a.b`: sintaxis, raíz reservada, bloque previo
/// existente y ruta verificable contra su `outputSchema` (+ tipos si el
/// destino declara el campo).
fn check_ref(
    instances: &[Instance],
    index: usize,
    source: &str,
    rel: &str,
    raw: &str,
    issues: &mut Vec<Issue>,
) {
    let path = match RefPath::parse(raw) {
        Ok(path) => path,
        Err(error) => {
            issues.push(Issue::new(field(index, source, rel), format!("{error}")));
            return;
        }
    };
    let segments = path.segments();
    let Some(Segment::Key(root)) = segments.first() else {
        issues.push(Issue::new(
            field(index, source, rel),
            format!("la referencia \"{raw}\" no puede empezar por un índice"),
        ));
        return;
    };
    // Raíces reservadas: el trigger y el reloj los pone el host, no hay schema
    // que verificar.
    if root == "trigger" || root == "now" {
        return;
    }
    let origin = match instances[..index].iter().find(|other| other.name == *root) {
        Some(origin) => origin,
        None => {
            let message = if instances[index + 1..]
                .iter()
                .any(|other| other.name == *root)
            {
                format!(
                    "la referencia \"{raw}\" apunta a \"{root}\", que se ejecuta después: \
                     la cadena es lineal y solo puede referenciar bloques previos"
                )
            } else {
                format!(
                    "la referencia \"{raw}\" apunta al bloque \"{root}\", que no existe en la cadena"
                )
            };
            issues.push(Issue::new(field(index, source, rel), message));
            return;
        }
    };
    match resolve_path(&origin.output_schema, &segments[1..]) {
        PathResolution::Unknown => {}
        PathResolution::Missing(detail) => {
            issues.push(Issue::new(field(index, source, rel), detail));
        }
        PathResolution::Found(node) => {
            check_type(
                instance_of(instances, index),
                index,
                source,
                rel,
                raw,
                node,
                issues,
            );
        }
    }
}

/// La instancia destino (la que declara la ref); `index` es siempre válido
/// porque `walk_refs` solo itera instancias existentes.
fn instance_of(instances: &[Instance], index: usize) -> &Instance {
    &instances[index]
}

/// Compatibilidad de tipos campo a campo: el tipo del nodo origen (ya
/// verificado) contra el tipo declarado para ese campo en el `inputSchema`
/// del destino. Sin tipos declarados en cualquiera de los dos lados no hay
/// nada que comparar.
fn check_type(
    instance: &Instance,
    index: usize,
    source: &str,
    rel: &str,
    raw: &str,
    produced: &Value,
    issues: &mut Vec<Issue>,
) {
    // La config es una plantilla de texto (URLs, headers): no hay schema de
    // destino que comparar.
    if source != "input" {
        return;
    }
    let Some(dest_segments) = key_segments(rel) else {
        return;
    };
    if let PathResolution::Found(expected) = resolve_path(&instance.input_schema, &dest_segments) {
        let produced_type = produced.get("type").and_then(Value::as_str);
        let expected_type = expected.get("type").and_then(Value::as_str);
        if let (Some(from), Some(to)) = (produced_type, expected_type) {
            // ensanchamiento: integer cabe en number
            let compatible = from == to || (from == "integer" && to == "number");
            if !compatible {
                issues.push(Issue::new(
                    field(index, source, rel),
                    format!("\"{raw}\" produce {from} pero el campo espera {to}"),
                ));
            }
        }
    }
}

/// Convierte la ruta relativa del mapeo (`.total`, `.cliente.email`) en
/// segmentos de clave para recorrer el `inputSchema` del destino. Un índice
/// numérico corta la comparación: los arrays rara vez declaran su forma en el
/// destino y no hay nada razonable que verificar.
fn key_segments(rel: &str) -> Option<Vec<Segment>> {
    let mut segments = Vec::new();
    for part in rel.split('.').filter(|part| !part.is_empty()) {
        if part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        segments.push(Segment::Key(part.to_string()));
    }
    Some(segments)
}

/// Resuelve las instancias del borrador contra la librería de bloques.
///
/// Devuelve las instancias (posiblemente incompletas cuando hubo errores) más
/// los issues de resolución: campos faltantes, UUIDs inválidos o versiones que
/// no existen en este workspace. Una instancia no resuelta sigue participando
/// en `validate_chain` con schemas vacíos, para acumular el resto de errores.
///
/// La resolución NO valida `id`/`name`: eso lo reporta `validate_chain` (que
/// conoce posiciones y duplicados), para no duplicar errores.
/// Valida el enrutado condicional del borrador (MVP2): `when` conforme a la
/// gramática declarativa y bloques `switch` bien formados.
///
/// Contrato del switch: instancia cuya config declara `routes`
/// (array de `{when, to}`) con `default` opcional. Cada `to` es el id de una
/// instancia existente y POSTERIOR (un salto hacia atrás formaría un ciclo);
/// toda condición es de la gramática declarativa — se rechaza JS arbitrario —
/// y la versión instanciada no puede llevar código (el switch es built-in).
fn validate_conditional(instances: &[Instance]) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (index, instance) in instances.iter().enumerate() {
        let prefix = format!("blocks[{index}]");
        if let Some(when) = &instance.when
            && let Err(error) = domain::conditions::validate(when)
        {
            issues.push(Issue::new(
                format!("{prefix}.when"),
                format!(
                    "las condiciones son declarativas (gramática eq/ne/gt/gte/lt/lte/exists/contains + and/or/not): {error}"
                ),
            ));
        }

        let Some(routes) = instance.config.get("routes") else {
            continue;
        };
        if instance.code.is_some() {
            issues.push(Issue::new(
                format!("{prefix}.config.routes"),
                "el switch es un bloque built-in sin código: la versión instanciada no puede declarar código",
            ));
            continue;
        }
        let Some(items) = routes.as_array() else {
            issues.push(Issue::new(
                format!("{prefix}.config.routes"),
                "\"routes\" debe ser un array de rutas {when, to}",
            ));
            continue;
        };
        for (route_index, route) in items.iter().enumerate() {
            let route_field = format!("{prefix}.config.routes[{route_index}]");
            let Some(when) = route.get("when") else {
                issues.push(Issue::new(
                    format!("{route_field}.when"),
                    "cada ruta necesita \"when\" (condición declarativa)",
                ));
                continue;
            };
            if let Err(error) = domain::conditions::validate(when) {
                issues.push(Issue::new(
                    format!("{route_field}.when"),
                    format!(
                        "las condiciones son declarativas (gramática eq/ne/gt/gte/lt/lte/exists/contains + and/or/not): {error}"
                    ),
                ));
            }
            check_route_target(
                instances,
                index,
                &route_field,
                route.get("to").and_then(Value::as_str),
                &mut issues,
            );
        }
        if let Some(default_to) = instance.config.get("default").and_then(Value::as_str) {
            check_route_target(
                instances,
                index,
                &format!("{prefix}.config.default"),
                Some(default_to),
                &mut issues,
            );
        } else if items.is_empty() {
            issues.push(Issue::new(
                format!("{prefix}.config.routes"),
                "un switch sin rutas necesita \"default\" (si no, nunca decide nada)",
            ));
        }
    }
    issues
}

/// Comprueba que el `to` de una ruta apunta a una instancia existente y
/// posterior al switch (un salto hacia atrás formaría un ciclo).
fn check_route_target(
    instances: &[Instance],
    switch_index: usize,
    field: &str,
    target: Option<&str>,
    issues: &mut Vec<Issue>,
) {
    let Some(target_id) = target else {
        issues.push(Issue::new(
            field.to_string(),
            "la ruta necesita \"to\": el id de la instancia destino",
        ));
        return;
    };
    match instances.iter().position(|candidate| candidate.id == target_id) {
        Some(at) if at > switch_index => {}
        Some(at) => issues.push(Issue::new(
            field.to_string(),
            format!(
                "la ruta salta hacia atrás: \"{target_id}\" es blocks[{at}] y el switch es blocks[{switch_index}] (formaría un ciclo)"
            ),
        )),
        None => issues.push(Issue::new(
            field.to_string(),
            format!("la ruta apunta a \"{target_id}\", que no existe en la cadena"),
        )),
    }
}

async fn resolve_instances(
    conn: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    document: &Value,
) -> (Vec<Instance>, Vec<Issue>) {
    let mut instances = Vec::new();
    let mut issues = Vec::new();
    let Some(entries) = document.get("blocks").filter(|value| !value.is_null()) else {
        // Sin bloques declarados: `validate_chain` reporta la cadena vacía.
        return (instances, issues);
    };
    let Some(entries) = entries.as_array() else {
        issues.push(Issue::new(
            "blocks",
            "\"blocks\" debe ser un array de instancias",
        ));
        return (instances, issues);
    };
    // (posición, id de block_version) a resolver contra la librería.
    let mut referenced: Vec<(usize, Uuid)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(entry) = entry.as_object() else {
            issues.push(Issue::new(
                format!("blocks[{index}]"),
                "cada instancia debe ser un objeto",
            ));
            continue;
        };
        let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_string);
        let instance = Instance {
            id: text("id").unwrap_or_default(),
            name: text("name").unwrap_or_default(),
            block_id: text("blockId").unwrap_or_default(),
            block_version_id: text("blockVersionId").unwrap_or_default(),
            input: entry
                .get("input")
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or_else(|| json!({})),
            config: entry
                .get("config")
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or_else(|| json!({})),
            input_schema: json!({}),
            output_schema: json!({}),
            code: None,
            when: entry.get("when").filter(|value| !value.is_null()).cloned(),
            timeout_ms: entry
                .get("timeoutMs")
                .filter(|value| !value.is_null())
                .and_then(Value::as_u64),
            resolved: false,
        };
        if instance.block_id.is_empty() {
            issues.push(Issue::new(
                format!("blocks[{index}].blockId"),
                "cada instancia necesita el blockId del bloque que instancia",
            ));
        }
        if instance.block_version_id.is_empty() {
            issues.push(Issue::new(
                format!("blocks[{index}].blockVersionId"),
                "cada instancia necesita la blockVersionId exacta que usa",
            ));
        } else if let (Ok(_), Ok(version_id)) = (
            Uuid::parse_str(&instance.block_id),
            Uuid::parse_str(&instance.block_version_id),
        ) {
            referenced.push((index, version_id));
        } else if !instance.block_id.is_empty() {
            issues.push(Issue::new(
                format!("blocks[{index}].blockVersionId"),
                "blockId y blockVersionId deben ser UUIDs",
            ));
        }
        instances.push(instance);
    }
    if referenced.is_empty() {
        return (instances, issues);
    }
    let ids: Vec<Uuid> = referenced.iter().map(|(_, id)| *id).collect();
    // Las block_versions se resuelven siempre scoped al workspace del puente:
    // la key no puede instanciar bloques de otro ni colarse por UUID.
    let rows = sqlx::query_as::<_, (Uuid, Option<String>, Value, Value)>(
        "SELECT bv.id, bv.code, bv.input_schema, bv.output_schema \
         FROM block_versions bv JOIN blocks b ON b.id = bv.block_id \
         WHERE b.workspace_id = $1 AND bv.id = ANY($2)",
    )
    .bind(workspace_id)
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await;
    let rows = match rows {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(%error, "fallo al resolver las block_versions del borrador");
            issues.push(Issue::new(
                "blocks",
                "fallo de base de datos al resolver los bloques instanciados",
            ));
            return (instances, issues);
        }
    };
    let by_id: HashMap<Uuid, (Option<String>, Value, Value)> = rows
        .into_iter()
        .map(|(id, code, input_schema, output_schema)| (id, (code, input_schema, output_schema)))
        .collect();
    for (index, version_id) in referenced {
        let Some(instance) = instances.get_mut(index) else {
            continue;
        };
        match by_id.get(&version_id) {
            Some((code, input_schema, output_schema)) => {
                instance.code = code.clone();
                instance.input_schema = input_schema.clone();
                instance.output_schema = output_schema.clone();
                instance.resolved = true;
            }
            None => issues.push(Issue::new(
                format!("blocks[{index}].blockVersionId"),
                "la versión del bloque no existe en este workspace",
            )),
        }
    }
    (instances, issues)
}

#[allow(dead_code)]
fn not_found() -> Problem {
    crate::problem::not_found("el puente no existe en este workspace")
}

// ---------------------------------------------------------------- Router ---

/// Rutas de puentes bajo `/api/v1`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/bridges", post(create_bridge).get(list_bridges))
        .route(
            "/bridges/{id}",
            get(get_bridge).patch(patch_bridge).delete(delete_bridge),
        )
        .route("/bridges/{id}/validate", post(validate_bridge))
        .route(
            "/bridges/{id}/versions",
            post(publish_version).get(list_versions),
        )
        .route("/bridges/{id}/webhook-secret", get(get_webhook_secret))
        .route("/bridges/{id}/run", post(run_bridge))
        .route("/bridges/{id}/dry-run", post(dry_run))
}

// ------------------------------------------------------------ Handlers ---

/// Columnas del DTO de puente (en el orden que espera `BridgeDto`).
const BRIDGE_COLUMNS: &str =
    "id, slug, name, description, latest_version, draft, created_at, updated_at";

/// Un puente tal como lo expone la API.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct BridgeDto {
    id: Uuid,
    slug: String,
    name: String,
    description: String,
    latest_version: Option<i32>,
    draft: Option<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Una versión publicada de un puente (el snapshot completo queda en
/// `bridge_versions.definition`; el listado devuelve metadatos).
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct BridgeVersionDto {
    id: Uuid,
    version: i32,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateBridge {
    slug: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    trigger: Option<Value>,
    #[serde(default)]
    input_schema: Option<Value>,
    #[serde(default)]
    blocks: Option<Value>,
}

/// Construye el borrador inicial desde las secciones del body, descartando
/// los `null` explícitos.
fn draft_from_parts(
    trigger: Option<Value>,
    input_schema: Option<Value>,
    blocks: Option<Value>,
) -> Value {
    let mut draft = serde_json::Map::new();
    for (key, value) in [
        ("trigger", trigger),
        ("inputSchema", input_schema),
        ("blocks", blocks),
    ] {
        if let Some(value) = value.filter(|value| !value.is_null()) {
            draft.insert(key.to_string(), value);
        }
    }
    Value::Object(draft)
}

/// Valida la FORMA del borrador (tipos de cada sección), no su semántica:
/// los errores semánticos los reporta el validador de cadena. Sirve tanto
/// para crear como para editar el borrador.
fn draft_shape_issues(draft: &Value) -> Vec<(String, String)> {
    let mut issues = Vec::new();
    if let Some(trigger) = draft.get("trigger").filter(|value| !value.is_null())
        && !trigger.is_object()
    {
        issues.push((
            "trigger".to_string(),
            "\"trigger\" debe ser un objeto".to_string(),
        ));
    }
    if let Some(schema) = draft.get("inputSchema").filter(|value| !value.is_null())
        && !schema.is_object()
    {
        issues.push((
            "inputSchema".to_string(),
            "\"inputSchema\" debe ser un objeto".to_string(),
        ));
    }
    if let Some(blocks) = draft.get("blocks").filter(|value| !value.is_null()) {
        let Some(entries) = blocks.as_array() else {
            issues.push((
                "blocks".to_string(),
                "\"blocks\" debe ser un array".to_string(),
            ));
            return issues;
        };
        for (index, entry) in entries.iter().enumerate() {
            let Some(entry) = entry.as_object() else {
                issues.push((
                    format!("blocks[{index}]"),
                    "cada instancia debe ser un objeto".to_string(),
                ));
                continue;
            };
            for key in ["id", "name", "blockId", "blockVersionId"] {
                if let Some(value) = entry.get(key)
                    && !value.is_null()
                    && value.as_str().is_none()
                {
                    issues.push((
                        format!("blocks[{index}].{key}"),
                        format!("\"{key}\" debe ser un string"),
                    ));
                }
            }
            for key in ["input", "config"] {
                if let Some(value) = entry.get(key)
                    && !value.is_null()
                    && !value.is_object()
                {
                    issues.push((
                        format!("blocks[{index}].{key}"),
                        format!("\"{key}\" debe ser un objeto"),
                    ));
                }
            }
        }
    }
    issues
}

/// `POST /api/v1/bridges`: crea el puente con su borrador inicial.
///
/// Solo valida la FORMA del borrador (tipos); la semántica (cadena, refs,
/// schemas) se valida con `POST /bridges/{id}/validate` y al publicar.
#[tracing::instrument(skip_all)]
async fn create_bridge(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    body: Bytes,
) -> Result<Response, Problem> {
    let workspace_id = key.workspace()?;
    let request: CreateBridge = rest::parse_body(&body)?;
    if let Err(message) = rest::check_slug(&request.slug) {
        return Err(rest::invalid_body(
            "el body no es un puente válido",
            vec![rest::field_error("slug", message)],
        ));
    }
    if let Err(message) = rest::check_text("name", &request.name, 200) {
        return Err(rest::invalid_body(
            "el body no es un puente válido",
            vec![rest::field_error("name", message)],
        ));
    }
    if request.description.chars().count() > 2000 {
        return Err(rest::invalid_body(
            "el body no es un puente válido",
            vec![rest::field_error("description", "supera 2000 caracteres")],
        ));
    }
    let draft = draft_from_parts(request.trigger, request.input_schema, request.blocks);
    let shape_issues = draft_shape_issues(&draft);
    if !shape_issues.is_empty() {
        return Err(rest::invalid_body(
            "el body no es un puente válido",
            shape_issues
                .into_iter()
                .map(|(field, message)| rest::field_error(&field, message))
                .collect(),
        ));
    }
    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, BridgeDto>(&format!(
        "INSERT INTO bridges (id, workspace_id, slug, name, description, draft) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING {BRIDGE_COLUMNS}"
    ))
    .bind(id)
    .bind(workspace_id)
    .bind(&request.slug)
    .bind(&request.name)
    .bind(&request.description)
    .bind(&draft)
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        if rest::is_unique_violation(&error) {
            problem::conflict(format!(
                "el slug \"{}\" ya existe en este workspace",
                request.slug
            ))
        } else {
            rest::db_error("crear el puente", error)
        }
    })?;
    Ok(rest::created(&format!("/api/v1/bridges/{id}"), Json(row)))
}

/// `GET /api/v1/bridges`: listado paginado por cursor (created_at, id), más
/// nuevo primero.
async fn list_bridges(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<BridgeDto>>, Problem> {
    let workspace_id = key.workspace()?;
    let limit = rest::page_limit(query.limit.as_deref())?;
    let after = match query.cursor.as_deref() {
        Some(raw) => Some(
            cursor::decode_bridge_cursor(raw)
                .map_err(|_| problem::bad_request("el cursor es inválido"))?,
        ),
        None => None,
    };
    let mut rows = match after {
        Some((created_at, id)) => {
            sqlx::query_as::<_, BridgeDto>(&format!(
                "SELECT {BRIDGE_COLUMNS} FROM bridges WHERE workspace_id = $1 \
                 AND (created_at, id) < ($2, $3) \
                 ORDER BY created_at DESC, id DESC LIMIT $4"
            ))
            .bind(workspace_id)
            .bind(created_at)
            .bind(id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
        }
        None => {
            sqlx::query_as::<_, BridgeDto>(&format!(
                "SELECT {BRIDGE_COLUMNS} FROM bridges WHERE workspace_id = $1 \
                 ORDER BY created_at DESC, id DESC LIMIT $2"
            ))
            .bind(workspace_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
        }
    }
    .map_err(|error| rest::db_error("listar los puentes", error))?;
    let next_cursor = rest::next_cursor(&mut rows, limit, |bridge| {
        cursor::encode_bridge_cursor(bridge.created_at, bridge.id)
    });
    Ok(Json(Page {
        items: rows,
        next_cursor,
    }))
}

/// Carga un puente scoped al workspace (404 si no existe o es de otro).
async fn load_bridge(state: &AppState, workspace_id: Uuid, id: Uuid) -> Result<BridgeDto, Problem> {
    sqlx::query_as::<_, BridgeDto>(&format!(
        "SELECT {BRIDGE_COLUMNS} FROM bridges WHERE id = $1 AND workspace_id = $2"
    ))
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| rest::db_error("cargar el puente", error))?
    .ok_or_else(not_found)
}

/// `GET /api/v1/bridges/{id}`.
async fn get_bridge(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
) -> Result<Json<BridgeDto>, Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "el puente no existe en este workspace")?;
    Ok(Json(load_bridge(&state, workspace_id, id).await?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PatchBridge {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    trigger: Option<Value>,
    #[serde(default)]
    input_schema: Option<Value>,
    #[serde(default)]
    blocks: Option<Value>,
    /// Reglas de enmascaramiento (MVP3): array de rutas `$.`. `null` = no tocar.
    #[serde(default)]
    mask: Option<Value>,
    /// Eventos a emitir al completar (MVP3): `{onSuccess, onFailure}`. `null` = no tocar.
    #[serde(default)]
    emit: Option<Value>,
    /// Webhook de resultado (MVP2): `{url}` https. `null` = no tocar.
    #[serde(default)]
    webhook: Option<Value>,
}

/// `PATCH /api/v1/bridges/{id}`: edita nombre/descripción y REEMPLAZA por
/// completo las secciones del borrador que vengan en el body (las ausentes
/// quedan como estaban; la ausencia no borra una sección).
///
/// Solo valida la forma del borrador resultante; la semántica se reporta con
/// `POST /bridges/{id}/validate` y se exige al publicar.
#[tracing::instrument(skip_all)]
async fn patch_bridge(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
    body: Bytes,
) -> Result<Json<BridgeDto>, Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "el puente no existe en este workspace")?;
    let request: PatchBridge = rest::parse_body(&body)?;
    if let Some(name) = &request.name
        && let Err(message) = rest::check_text("name", name, 200)
    {
        return Err(rest::invalid_body(
            "el body no es un puente válido",
            vec![rest::field_error("name", message)],
        ));
    }
    if let Some(description) = &request.description
        && description.chars().count() > 2000
    {
        return Err(rest::invalid_body(
            "el body no es un puente válido",
            vec![rest::field_error("description", "supera 2000 caracteres")],
        ));
    }
    let current = load_bridge(&state, workspace_id, id).await?;
    let mut draft = current
        .draft
        .unwrap_or_else(|| json!({}))
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (key, value) in [
        ("trigger", request.trigger),
        ("inputSchema", request.input_schema),
        ("blocks", request.blocks),
        ("mask", request.mask),
        ("emit", request.emit),
        ("webhook", request.webhook),
    ] {
        if let Some(value) = value.filter(|value| !value.is_null()) {
            draft.insert(key.to_string(), value);
        }
    }
    let draft = Value::Object(draft);
    let shape_issues = draft_shape_issues(&draft);
    if !shape_issues.is_empty() {
        return Err(rest::invalid_body(
            "el body no es un puente válido",
            shape_issues
                .into_iter()
                .map(|(field, message)| rest::field_error(&field, message))
                .collect(),
        ));
    }
    let row = sqlx::query_as::<_, BridgeDto>(&format!(
        "UPDATE bridges SET name = COALESCE($3, name), description = COALESCE($4, description), \
         draft = $5, updated_at = now() \
         WHERE id = $1 AND workspace_id = $2 RETURNING {BRIDGE_COLUMNS}"
    ))
    .bind(id)
    .bind(workspace_id)
    .bind(&request.name)
    .bind(&request.description)
    .bind(&draft)
    .fetch_one(&state.pool)
    .await
    .map_err(|error| rest::db_error("actualizar el puente", error))?;
    Ok(Json(row))
}

/// `DELETE /api/v1/bridges/{id}`: borra el puente; sus versiones y
/// ejecuciones desaparecen por `ON DELETE CASCADE`. Devuelve 204 sin cuerpo.
#[tracing::instrument(skip_all)]
async fn delete_bridge(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
) -> Result<StatusCode, Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "el puente no existe en este workspace")?;
    let result = sqlx::query("DELETE FROM bridges WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(workspace_id)
        .execute(&state.pool)
        .await
        .map_err(|error| rest::db_error("borrar el puente", error))?;
    if result.rows_affected() == 0 {
        return Err(not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Valida un borrador completo sobre una conexión ya abierta (pool o
/// transacción): trigger, inputSchema, resolución de instancias y cadena.
///
/// Devuelve los issues y las instancias resueltas (estas solo interesan a la
/// publicación, que arma el snapshot con ellas).
async fn validar_borrador(
    conn: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    draft: Option<&Value>,
) -> Result<(Vec<Issue>, Vec<Instance>), Problem> {
    let Some(draft) = draft else {
        return Ok((
            vec![Issue::new(
                "draft",
                "el puente no tiene borrador todavía: envía trigger/inputSchema/blocks",
            )],
            Vec::new(),
        ));
    };
    let mut issues = validate_trigger(draft.get("trigger").filter(|value| !value.is_null()));
    // Webhook de resultado (MVP2): la entrega viaja firmada y solo por TLS.
    if let Some(webhook) = draft.get("webhook").filter(|value| !value.is_null()) {
        match webhook.get("url").and_then(Value::as_str) {
            None => issues.push(Issue::new(
                "webhook.url",
                "el webhook necesita \"url\" (la https donde se entrega el resultado)",
            )),
            Some(url) if !url.starts_with("https://") => {
                issues.push(Issue::new(
                    "webhook.url",
                    format!(
                        "\"{url}\" debe ser https: la entrega del resultado viaja firmada y solo se envía por TLS"
                    ),
                ));
            }
            Some(_) => {}
        }
    }
    if let Some(schema) = draft.get("inputSchema").filter(|value| !value.is_null())
        && let Err(error) = domain::schema::validate_schema_doc(schema)
    {
        issues.push(Issue::new(
            "inputSchema",
            format!("no es un schema del subconjunto soportado: {error}"),
        ));
    }
    let (instances, resolve_issues) = resolve_instances(conn, workspace_id, draft).await;
    issues.extend(resolve_issues);
    issues.extend(validate_chain(&instances));
    issues.extend(validate_conditional(&instances));
    let nombres: Vec<&str> = instances
        .iter()
        .map(|instancia| instancia.name.as_str())
        .collect();
    issues.extend(validate_mask(draft.get("mask"), &nombres));
    issues.extend(validate_emit(draft.get("emit")));
    Ok((issues, instances))
}

/// MVP3 (spec `event-triggers`): los topics de `emit` siguen el mismo
/// formato de slug que los triggers event; solo se aceptan las claves
/// `onSuccess`/`onFailure` con arrays de cadenas.
fn validate_emit(emit: Option<&Value>) -> Vec<Issue> {
    let Some(emit) = emit.filter(|valor| !valor.is_null()) else {
        return Vec::new();
    };
    let Some(objeto) = emit.as_object() else {
        return vec![Issue::new(
            "emit",
            "debe ser un objeto con \"onSuccess\"/\"onFailure\" (arrays de topics)",
        )];
    };
    let mut issues = Vec::new();
    for clave in objeto.keys() {
        if clave != "onSuccess" && clave != "onFailure" {
            issues.push(Issue::new(
                "emit",
                format!(
                    "\"{clave}\" no es una clave de emit (usa \"onSuccess\" y/o \"onFailure\")"
                ),
            ));
        }
    }
    for (clave, topics) in objeto {
        if clave != "onSuccess" && clave != "onFailure" {
            continue;
        }
        let Some(lista) = topics.as_array() else {
            issues.push(Issue::new(
                format!("emit.{clave}"),
                "debe ser un array de topics",
            ));
            continue;
        };
        for (indice, topic) in lista.iter().enumerate() {
            let campo = format!("emit.{clave}.{indice}");
            let Some(raw) = topic.as_str() else {
                issues.push(Issue::new(&campo, "cada topic debe ser una cadena"));
                continue;
            };
            if !domain::definition::validar_topic(raw) {
                issues.push(Issue::new(
                    &campo,
                    format!(
                        "\"{raw}\" no es un topic válido (minúsculas, dígitos, puntos y guiones)"
                    ),
                ));
            }
        }
    }
    issues
}

/// MVP3 (spec `io-masking`): las reglas `mask` son un array de rutas `$.`
/// cuya raíz es `trigger` o el nombre de un bloque de la cadena. Cualquier
/// otra cosa (raíz desconocida, ruta rota, tipo equivocado) es un issue.
fn validate_mask(mask: Option<&Value>, block_names: &[&str]) -> Vec<Issue> {
    let Some(mask) = mask.filter(|valor| !valor.is_null()) else {
        return Vec::new();
    };
    let Some(reglas) = mask.as_array() else {
        return vec![Issue::new(
            "mask",
            "debe ser un array de rutas \"$.\" (por ejemplo \"$.trigger.password\")",
        )];
    };
    let mut issues = Vec::new();
    for (indice, regla) in reglas.iter().enumerate() {
        let campo = format!("mask.{indice}");
        let Some(raw) = regla.as_str() else {
            issues.push(Issue::new(
                &campo,
                "cada regla debe ser una cadena con una ruta \"$.\"",
            ));
            continue;
        };
        let Ok(path) = RefPath::parse(raw) else {
            issues.push(Issue::new(
                &campo,
                format!("\"{raw}\" no es una ruta \"$.\" válida (raíz y segmentos alfanuméricos)"),
            ));
            continue;
        };
        let raiz_ok = match path.segments().first() {
            Some(Segment::Key(clave)) => {
                clave == "trigger" || block_names.contains(&clave.as_str())
            }
            _ => false,
        };
        if !raiz_ok {
            issues.push(Issue::new(
                &campo,
                format!(
                    "la raíz de \"{raw}\" debe ser \"trigger\" o el nombre de un bloque de la cadena"
                ),
            ));
        }
    }
    issues
}

/// `POST /api/v1/bridges/{id}/validate`: valida el borrador completo y
/// devuelve `200 {valid, errors[]}` (200 aunque haya errores: validar es el
/// propósito del endpoint).
async fn validate_bridge(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
) -> Result<Json<Value>, Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "el puente no existe en este workspace")?;
    let bridge = load_bridge(&state, workspace_id, id).await?;
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|error| rest::db_error("conectar a la base de datos", error))?;
    let (issues, _) = validar_borrador(&mut conn, workspace_id, bridge.draft.as_ref()).await?;
    Ok(Json(json!({
        "valid": issues.is_empty(),
        "errors": issues.iter().map(Issue::to_json).collect::<Vec<_>>(),
    })))
}

/// `POST /api/v1/bridges/{id}/versions`: valida el borrador y congela la
/// siguiente versión con el snapshot resuelto (cuerpo vacío: todo lo
/// publicable vive en el borrador).
///
/// La asignación de versión es atómica: `FOR UPDATE` sobre el puente dentro
/// de la transacción serializa publicaciones concurrentes y evita repetir
/// número; `UNIQUE(bridge_id, version)` es la última defensa.
#[tracing::instrument(skip_all)]
/// Construye la `BridgeDefinition` tipada del borrador ya VALIDADO (trigger
/// congelado, instancias resueltas con su code, webhook/mask/emit). La usan
/// la publicación (para el snapshot) y el dry-run (para ensayar el borrador
/// sin publicar), garantizando que ambos ven exactamente la misma definición.
fn definicion_del_borrador(
    draft: &Value,
    instances: &[Instance],
) -> Result<BridgeDefinition, Problem> {
    let trigger: TriggerDefinition = serde_json::from_value(
        draft
            .get("trigger")
            .cloned()
            .expect("validate_trigger garantiza el trigger"),
    )
    .map_err(|error| {
        rest::invalid_body(
            "el borrador no pasa la validación",
            vec![rest::field_error("trigger", error.to_string())],
        )
    })?;
    let blocks: Vec<BlockInstance> = instances
        .iter()
        .map(|instance| BlockInstance {
            id: instance.id.clone(),
            name: instance.name.clone(),
            block_id: Some(instance.block_id.clone()),
            block_version_id: Some(instance.block_version_id.clone()),
            code: instance.code.clone(),
            input: instance.input.clone(),
            config: instance.config.clone(),
            input_schema: instance.input_schema.clone(),
            output_schema: instance.output_schema.clone(),
            when: instance.when.clone(),
            timeout_ms: instance.timeout_ms,
        })
        .collect();
    let webhook: Option<domain::definition::WebhookDefinition> = draft
        .get("webhook")
        .filter(|value| !value.is_null())
        .cloned()
        .map(serde_json::from_value::<domain::definition::WebhookDefinition>)
        .transpose()
        .map_err(|error| {
            rest::invalid_body(
                "el borrador no pasa la validación",
                vec![rest::field_error("webhook", error.to_string())],
            )
        })?;
    let mask: Vec<String> = draft
        .get("mask")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<Vec<String>>(value.clone()))
        .transpose()
        .map_err(|error| {
            rest::invalid_body(
                "el borrador no pasa la validación",
                vec![rest::field_error("mask", error.to_string())],
            )
        })?
        .unwrap_or_default();
    let emit: Option<domain::definition::EmitDefinition> = draft
        .get("emit")
        .filter(|value| !value.is_null())
        .map(|valor| serde_json::from_value::<domain::definition::EmitDefinition>(valor.clone()))
        .transpose()
        .map_err(|error| {
            rest::invalid_body(
                "el borrador no pasa la validación",
                vec![rest::field_error("emit", error.to_string())],
            )
        })?;
    Ok(BridgeDefinition {
        trigger,
        input_schema: draft
            .get("inputSchema")
            .filter(|value| !value.is_null())
            .cloned(),
        blocks,
        webhook: webhook.clone(),
        mask,
        emit,
    })
}

async fn publish_version(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
    body: Bytes,
) -> Result<Response, Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "el puente no existe en este workspace")?;
    if !body.is_empty() {
        return Err(problem::bad_request(
            "la publicación usa el borrador del puente: el body debe estar vacío",
        ));
    }
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| rest::db_error("abrir la transacción", error))?;
    let bridge = sqlx::query_as::<_, BridgeDto>(&format!(
        "SELECT {BRIDGE_COLUMNS} FROM bridges WHERE id = $1 AND workspace_id = $2 FOR UPDATE"
    ))
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| rest::db_error("cargar el puente", error))?
    .ok_or_else(not_found)?;
    let (issues, instances) =
        validar_borrador(&mut tx, workspace_id, bridge.draft.as_ref()).await?;
    if !issues.is_empty() {
        return Err(rest::invalid_body(
            "el borrador no pasa la validación",
            issues.iter().map(Issue::to_json).collect(),
        ));
    }
    // El snapshot: trigger tipado, inputSchema y las instancias RESUELTAS
    // (code copiado de la block_version y schemas propios).
    let draft = bridge
        .draft
        .expect("validar_borrador ya garantiza el borrador");
    let definition_webhook_presente = draft
        .get("webhook")
        .filter(|value| !value.is_null())
        .is_some();
    let definition = definicion_del_borrador(&draft, &instances)?;
    let definition = serde_json::to_value(&definition)
        .map_err(|error| problem::internal(format!("snapshot no serializable: {error}")))?;
    // Versión siguiente atómica: el lock del puente serializa publicaciones.
    let version: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM bridge_versions WHERE bridge_id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| rest::db_error("calcular la versión siguiente", error))?;
    let version_id = Uuid::now_v7();
    // Secreto HMAC del webhook (MVP2): se genera UNA vez al publicar la
    // primera versión con webhook y se conserva entre versiones (regenerarlo
    // rompería la verificación de los consumidores). Cifrado en reposo con la
    // envolvente de la bóveda; solo sale por el plano de administración.
    if definition_webhook_presente {
        let secret_actual: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT webhook_secret FROM bridges WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|error| rest::db_error("cargar el secreto del webhook", error))?;
        if secret_actual.is_none() {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes)
                .map_err(|error| problem::internal(format!("generar el secreto: {error}")))?;
            let secret_hex = hex::encode(bytes);
            sqlx::query("UPDATE bridges SET webhook_secret = $2 WHERE id = $1")
                .bind(id)
                .bind(crate::vault::encrypt(&secret_hex))
                .execute(&mut *tx)
                .await
                .map_err(|error| rest::db_error("guardar el secreto del webhook", error))?;
        }
    }
    sqlx::query(
        "INSERT INTO bridge_versions (id, bridge_id, version, definition) VALUES ($1, $2, $3, $4)",
    )
    .bind(version_id)
    .bind(id)
    .bind(version)
    .bind(&definition)
    .execute(&mut *tx)
    .await
    .map_err(|error| rest::db_error("guardar la versión", error))?;
    sqlx::query("UPDATE bridges SET latest_version = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(version)
        .execute(&mut *tx)
        .await
        .map_err(|error| rest::db_error("actualizar el puente", error))?;
    tx.commit()
        .await
        .map_err(|error| rest::db_error("confirmar la publicación", error))?;
    Ok(rest::created(
        &format!("/api/v1/bridges/{id}/versions/{version}"),
        Json(json!({
            "id": version_id,
            "bridgeId": id,
            "version": version,
            "definition": definition,
        })),
    ))
}

/// `GET /api/v1/bridges/{id}/versions`: versiones publicadas, más nueva
/// primero, paginadas por número de versión.
async fn list_versions(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<BridgeVersionDto>>, Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "el puente no existe en este workspace")?;
    let limit = rest::page_limit(query.limit.as_deref())?;
    let after_version = match query.cursor.as_deref() {
        Some(raw) => Some(
            cursor::decode_version_cursor(raw)
                .map_err(|_| problem::bad_request("el cursor es inválido"))?,
        ),
        None => None,
    };
    let rows = match after_version {
        Some(version) => {
            sqlx::query_as::<_, BridgeVersionDto>(
                "SELECT bv.id, bv.version, bv.created_at \
                 FROM bridge_versions bv JOIN bridges b ON b.id = bv.bridge_id \
                 WHERE bv.bridge_id = $1 AND b.workspace_id = $2 AND bv.version < $3 \
                 ORDER BY bv.version DESC LIMIT $4",
            )
            .bind(id)
            .bind(workspace_id)
            .bind(version)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
        }
        None => {
            sqlx::query_as::<_, BridgeVersionDto>(
                "SELECT bv.id, bv.version, bv.created_at \
                 FROM bridge_versions bv JOIN bridges b ON b.id = bv.bridge_id \
                 WHERE bv.bridge_id = $1 AND b.workspace_id = $2 \
                 ORDER BY bv.version DESC LIMIT $3",
            )
            .bind(id)
            .bind(workspace_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
        }
    }
    .map_err(|error| rest::db_error("listar las versiones", error))?;
    let mut rows = rows;
    let next_cursor = rest::next_cursor(&mut rows, limit, |version| {
        cursor::encode_version_cursor(version.version)
    });
    Ok(Json(Page {
        items: rows,
        next_cursor,
    }))
}

/// `GET /bridges/{id}/webhook-secret`: expone el secreto HMAC del puente,
/// por el plano de administración y solo por él (change MVP2).
///
/// El secreto es el mismo por puente (estable se haya publicado; regenerarlo
/// rompería la verificación de los consumidores). Sin webhook publicado
/// responde 404 Problem, y copiarlo requiere la auth Bearer de administración.
async fn get_webhook_secret(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
) -> Result<Json<Value>, Problem> {
    let id = rest::parse_id(&raw_id, "el puente no existe")?;
    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM bridges WHERE id = $1 AND workspace_id = $2")
            .bind(id)
            .bind(key.workspace_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|error| problem::internal(format!("cargar el puente: {error}")))?;
    if exists.is_none() {
        return Err(problem::not_found("el puente no existe"));
    }

    let secreto: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT webhook_secret FROM bridges WHERE id = $1")
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .map_err(|error| problem::internal(format!("cargar el secreto: {error}")))?;

    let Some(bytes) = secreto else {
        return Err(problem::not_found("el puente no tiene webhook publicado"));
    };
    let secreto_en_claro = crate::vault::decrypt(&bytes)
        .map_err(|error| problem::internal(format!("descifrar el secreto: {error}")))?;

    Ok(Json(json!({
        "secret": secreto_en_claro,
    })))
}

/// `POST /bridges/{id}/run`: disparo manual inmediato (MVP2, spec
/// `cron-triggers`): encola una ejecución `manual` de la versión publicada
/// activa con `run_at` inmediato y devuelve su `executionId`. Sin versión
/// publicada → 409 Problem (el worker la correrá en cuanto la reclame).
/// Cuerpo de `POST /bridges/{id}/dry-run`: datos de prueba para `$.trigger`
/// y mocks por instancia (`{id|nombre: {output}}`).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DryRunRequest {
    #[serde(default)]
    trigger_data: Option<Value>,
    #[serde(default)]
    mocks: Option<Value>,
}

/// `POST /api/v1/bridges/{id}/dry-run` (MVP3, spec `dry-run`): ejecuta el
/// BORRADOR actual — sin requerir versión publicada — con las mismas reglas
/// del ejecutor y devuelve el trace en la propia respuesta. Sin persistencia:
/// ni `executions` ni `execution_steps`, sin webhook y sin `emit`; el
/// resultado vive solo en la respuesta. Un bloque mockeado no se invoca.
async fn dry_run(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
    body: Bytes,
) -> Result<Json<Value>, Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "el puente no existe en este workspace")?;
    let cuerpo: DryRunRequest = if body.is_empty() {
        DryRunRequest {
            trigger_data: None,
            mocks: None,
        }
    } else {
        rest::parse_body(&body)?
    };

    // Mocks: mapa {id|nombre: {output}}; validar forma antes de correr.
    let mut mocks = std::collections::HashMap::new();
    if let Some(especificacion) = cuerpo.mocks.filter(|valor| !valor.is_null()) {
        let Some(objeto) = especificacion.as_object() else {
            return Err(rest::invalid_body(
                "el body no es un dry-run válido",
                vec![rest::field_error(
                    "mocks",
                    "debe ser un objeto {instancia: {output}}",
                )],
            ));
        };
        for (clave, valor) in objeto {
            let Some(salida) = valor.get("output") else {
                return Err(rest::invalid_body(
                    "el body no es un dry-run válido",
                    vec![rest::field_error(
                        format!("mocks.{clave}").as_str(),
                        "necesita \"output\" (la salida fija del mock)",
                    )],
                ));
            };
            mocks.insert(clave.clone(), salida.clone());
        }
    }

    // El borrador debe validar completo (mismas reglas que la publicación).
    let bridge = load_bridge(&state, workspace_id, id).await?;
    if bridge
        .draft
        .as_ref()
        .and_then(Value::as_object)
        .is_none_or(serde_json::Map::is_empty)
    {
        return Err(rest::invalid_body(
            "el borrador está vacío",
            vec![rest::field_error(
                "draft",
                "nada que ensayar: edita el borrador primero",
            )],
        ));
    }
    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|error| rest::db_error("conectar a la base de datos", error))?;
    let (issues, instances) =
        validar_borrador(&mut conn, workspace_id, bridge.draft.as_ref()).await?;
    if !issues.is_empty() {
        return Err(rest::invalid_body(
            "el borrador no pasa la validación",
            issues.iter().map(Issue::to_json).collect(),
        ));
    }
    drop(conn);
    let draft = bridge
        .draft
        .expect("validar_borrador ya garantiza el borrador");
    let definition = definicion_del_borrador(&draft, &instances)?;

    // `$.trigger` según el tipo congelado: http normaliza {body, query};
    // los demás reciben los datos de prueba tal cual.
    let datos = cuerpo.trigger_data.unwrap_or(Value::Null);
    let trigger_data = if definition.trigger.trigger_type == domain::definition::TriggerType::Http {
        json!({ "body": datos, "query": {} })
    } else {
        datos
    };

    // Bóveda real para los bloques NO mockeados (mismo camino de producción).
    let boveda = crate::invoke::cargar_boveda(&state.pool, workspace_id)
        .await
        .map_err(|error| problem::internal(format!("cargar la bóveda: {error}")))?;
    let resolutor = std::sync::Arc::new(move |name: String| {
        boveda.get(&name).map(|(secreto, _, _)| secreto.clone())
    });

    // La corrida en memoria: mismo ejecutor, mocks por instancia, sin BD.
    let corrida = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime current-thread para el ejecutor");
        let local = tokio::task::LocalSet::new();
        local.block_on(&runtime, async {
            let sandbox =
                engine::invoker::SandboxInvoker::new().with_credential_resolver(resolutor);
            let invoker = engine::invoker::MockingInvoker::new(sandbox, mocks);
            let executor = engine::executor::Executor::new(invoker);
            executor
                .run(engine::executor::ChainRun::new(&definition, trigger_data))
                .await
        })
    })
    .await
    .expect("hilo blocking del dry-run");

    Ok(Json(json!({
        "status": corrida.status.as_str(),
        "output": corrida.output,
        "error": corrida.error.as_ref().map(|error| serde_json::to_value(error).unwrap_or(Value::Null)),
        "steps": corrida.steps.iter().map(|paso| json!({
            "seq": paso.seq,
            "blockName": paso.block_name,
            "status": paso.status.as_str(),
            "input": paso.input,
            "output": paso.output,
            "error": paso.error.as_ref().map(|error| serde_json::to_value(error).unwrap_or(Value::Null)),
            "durationMs": paso.duration_ms,
        })).collect::<Vec<_>>(),
    })))
}

async fn run_bridge(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Path(raw_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), Problem> {
    let workspace_id = key.workspace()?;
    let id = rest::parse_id(&raw_id, "el puente no existe en este workspace")?;

    let Some((version_id,)): Option<(Uuid,)> = sqlx::query_as(
        "SELECT bv.id FROM bridges b \
         JOIN bridge_versions bv ON bv.bridge_id = b.id AND bv.version = b.latest_version \
         WHERE b.id = $1 AND b.workspace_id = $2 AND b.latest_version IS NOT NULL",
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("cargar la versión activa: {error}")))?
    else {
        return Err(
            Problem::new(StatusCode::CONFLICT, "no-published-version", "Conflict")
                .with_detail("el puente no tiene versión publicada: publícala antes de ejecutarlo"),
        );
    };

    let execution_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO executions \
             (id, workspace_id, bridge_id, bridge_version_id, trigger_type, trigger_data, \
              status, run_at) \
         VALUES ($1, $2, $3, $4, 'manual', '{}', 'pending', now())",
    )
    .bind(execution_id)
    .bind(workspace_id)
    .bind(id)
    .bind(version_id)
    .execute(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("encolar el disparo manual: {error}")))?;

    tracing::info!(execution = %execution_id, bridge = %id, "disparo manual encolado");
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "executionId": execution_id })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── fixtures ───────────────────────────────────────────────────────

    /// Schema estricto de salida del bloque `origen`.
    fn origen_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "total": { "type": "integer" },
                "cliente": {
                    "type": "object",
                    "properties": { "email": { "type": "string" } }
                },
                "items": {
                    "type": "array",
                    "items": { "type": "object", "properties": { "sku": { "type": "string" } } }
                }
            }
        })
    }

    fn instancia(name: &str, input: Value, output_schema: Value) -> Instance {
        Instance {
            id: format!("paso-{name}"),
            name: name.to_string(),
            block_id: Uuid::now_v7().to_string(),
            block_version_id: Uuid::now_v7().to_string(),
            input,
            config: json!({}),
            input_schema: json!({}),
            output_schema,
            code: Some("export default (input) => input".to_string()),
            when: None,
            timeout_ms: None,
            resolved: true,
        }
    }

    fn origen(input: Value) -> Instance {
        instancia("origen", input, origen_schema())
    }

    fn destino(input: Value, input_schema: Value) -> Instance {
        let mut instance = instancia("destino", input, json!({}));
        instance.input_schema = input_schema;
        instance
    }

    fn con_config(mut instance: Instance, config: Value) -> Instance {
        instance.config = config;
        instance
    }

    fn trigger_http() -> Value {
        json!({ "type": "http", "method": "POST", "path": "/x/demo" })
    }

    fn campos(issues: &[Issue]) -> Vec<&str> {
        issues.iter().map(|issue| issue.field.as_str()).collect()
    }

    fn mensajes(issues: &[Issue]) -> String {
        issues
            .iter()
            .map(|issue| format!("{}: {}", issue.field, issue.message))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn assert_sin_errores(issues: &[Issue]) {
        assert!(
            issues.is_empty(),
            "se esperaba válido: {}",
            mensajes(issues)
        );
    }

    // ── trigger ────────────────────────────────────────────────────────

    #[test]
    fn un_trigger_http_completo_es_valido() {
        assert_sin_errores(&validate_trigger(Some(&trigger_http())));
    }

    #[test]
    fn el_trigger_incompleto_se_reporta() {
        let cases: [(Option<Value>, &str); 6] = [
            (None, "trigger"),
            (Some(json!({})), "trigger.type"),
            (Some(json!({ "type": "pigeon" })), "trigger.type"),
            (Some(json!({ "type": "http" })), "trigger.method"),
            (
                Some(json!({ "type": "http", "method": "POST" })),
                "trigger.path",
            ),
            (
                Some(json!({ "type": "http", "method": "post", "path": "/x/demo" })),
                "trigger.method",
            ),
        ];
        for (trigger, field) in cases {
            let issues = validate_trigger(trigger.as_ref());
            assert!(
                campos(&issues).contains(&field),
                "se esperaba un error en {field}: {}",
                mensajes(&issues)
            );
        }
    }

    #[test]
    fn los_disparadores_no_http_solo_exigen_el_tipo() {
        // MVP3: el trigger event además exige topic con formato slug (MVP3).
        for trigger in [
            json!({ "type": "manual" }),
            json!({ "type": "event", "topic": "pedido.creado" }),
        ] {
            assert_sin_errores(&validate_trigger(Some(&trigger)));
        }
    }

    #[test]
    fn el_disparador_event_exige_topic_con_formato_slug() {
        assert_sin_errores(&validate_trigger(Some(&json!({
            "type": "event",
            "topic": "pedido.creado"
        }))));

        for trigger in [
            json!({ "type": "event" }),
            json!({ "type": "event", "topic": "Pedido Creado!!" }),
            json!({ "type": "event", "topic": ".empieza.punto" }),
            json!({ "type": "event", "topic": "doble..punto" }),
        ] {
            let issues = validate_trigger(Some(&trigger));
            assert!(
                issues.iter().any(|issue| issue.field == "trigger.topic"),
                "{trigger} debió exigir topic: {}",
                mensajes(&issues)
            );
        }
    }

    #[test]
    fn el_disparador_cron_exige_schedule_y_timezone_validos() {
        assert_sin_errores(&validate_trigger(Some(&json!({
            "type": "cron",
            "schedule": "*/5 * * * *",
            "timezone": "Europe/Madrid"
        }))));

        // Sin schedule/timezone, o con valores que no son cron/IANA.
        for trigger in [
            json!({ "type": "cron" }),
            json!({ "type": "cron", "schedule": "no-es-cron", "timezone": "UTC" }),
            json!({ "type": "cron", "schedule": "*/5 * * * *", "timezone": "Marte/Olympus" }),
        ] {
            let issues = validate_trigger(Some(&trigger));
            assert!(!issues.is_empty(), "{trigger}");
        }
    }

    // ── estructura ─────────────────────────────────────────────────────

    #[test]
    fn una_cadena_valida_no_reporta_errores() {
        let instances = vec![
            origen(json!({ "id": "$.trigger.body.id" })),
            destino(
                json!({ "id": "$.origen.id", "email": "$.origen.cliente.email" }),
                json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "email": { "type": "string" }
                    }
                }),
            ),
        ];
        assert_sin_errores(&validate_chain(&instances));
    }

    #[test]
    fn una_cadena_sin_bloques_es_un_error() {
        let issues = validate_chain(&[]);
        assert_eq!(campos(&issues), vec!["blocks"], "{}", mensajes(&issues));
        assert!(
            issues[0].message.contains("al menos un bloque"),
            "{}",
            mensajes(&issues)
        );
    }

    #[test]
    fn cada_instancia_necesita_id_y_nombre() {
        let mut rota = origen(json!({}));
        rota.id = String::new();
        rota.name = String::new();
        let issues = validate_chain(&[rota]);
        assert!(
            campos(&issues).contains(&"blocks[0].id"),
            "{}",
            mensajes(&issues)
        );
        assert!(
            campos(&issues).contains(&"blocks[0].name"),
            "{}",
            mensajes(&issues)
        );
    }

    #[test]
    fn ids_y_nombres_repetidos_son_errores() {
        let mut repetida = origen(json!({}));
        repetida.id = "paso-origen".to_string();
        repetida.name = "origen".to_string();
        let issues = validate_chain(&[origen(json!({})), repetida]);
        assert!(
            campos(&issues).contains(&"blocks[1].id"),
            "{}",
            mensajes(&issues)
        );
        assert!(
            campos(&issues).contains(&"blocks[1].name"),
            "{}",
            mensajes(&issues)
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("se pisarían")),
            "el nombre repetido explica la colisión en el contexto: {}",
            mensajes(&issues)
        );
    }

    #[test]
    fn un_nombre_reservado_es_un_error() {
        for reservado in ["trigger", "now"] {
            let issues = validate_chain(&[instancia(reservado, json!({}), json!({}))]);
            assert!(
                campos(&issues).contains(&"blocks[0].name"),
                "{reservado}: {}",
                mensajes(&issues)
            );
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.message.contains("reservado")),
                "{reservado}: {}",
                mensajes(&issues)
            );
        }
    }

    // ── refs: sintaxis y semántica ─────────────────────────────────────

    #[test]
    fn las_raices_reservadas_son_validas_sin_verificacion() {
        let instances = vec![origen(json!({
            "a": "$.trigger.body.cualquier",
            "b": "$.now",
            "c": { "anidada": ["$.trigger.body.lista.0.x", "$.now"] }
        }))];
        assert_sin_errores(&validate_chain(&instances));
    }

    #[test]
    fn una_ref_con_sintaxis_invalida_es_un_error() {
        let issues = validate_chain(&[origen(json!({ "a": "$.origen..id" }))]);
        assert_eq!(
            campos(&issues),
            vec!["blocks[0].input.a"],
            "{}",
            mensajes(&issues)
        );
        assert!(
            issues[0].message.contains("referencia"),
            "{}",
            mensajes(&issues)
        );
    }

    #[test]
    fn una_ref_a_un_bloque_inexistente_es_un_error() {
        let issues = validate_chain(&[origen(json!({ "a": "$.fantasma.id" }))]);
        assert_eq!(campos(&issues), vec!["blocks[0].input.a"]);
        assert!(
            issues[0].message.contains("fantasma"),
            "{}",
            mensajes(&issues)
        );
    }

    #[test]
    fn una_ref_a_un_bloque_posterior_rompe_la_cadena_lineal() {
        let instances = vec![
            origen(json!({ "a": "$.destino.id" })),
            destino(json!({}), json!({})),
        ];
        let issues = validate_chain(&instances);
        assert_eq!(campos(&issues), vec!["blocks[0].input.a"]);
        assert!(
            issues[0].message.contains("después"),
            "el mensaje explica que el bloque se ejecuta después: {}",
            mensajes(&issues)
        );
    }

    #[test]
    fn una_ref_a_una_ruta_verificable_e_inexistente_es_un_error() {
        for (input, needle) in [
            (json!({ "x": "$.origen.apellido" }), "apellido"),
            (json!({ "x": "$.origen.cliente.telefono" }), "telefono"),
            (json!({ "x": "$.origen.id.length" }), "length"),
            (json!({ "x": "$.origen.items.sku" }), "sku"),
        ] {
            let issues = validate_chain(&[origen(json!({})), destino(input, json!({}))]);
            assert_eq!(
                campos(&issues),
                vec!["blocks[1].input.x"],
                "{}",
                mensajes(&issues)
            );
            assert!(
                issues[0].message.contains(needle),
                "el mensaje nombra la ruta imposible ({needle}): {}",
                mensajes(&issues)
            );
        }
    }

    #[test]
    fn un_output_schema_suelto_no_bloquea_ni_avisa() {
        let libre = instancia("libre", json!({}), json!({}));
        let instances = vec![
            libre,
            destino(json!({ "x": "$.libre.lo.que.sea.0.profundo" }), json!({})),
        ];
        assert_sin_errores(&validate_chain(&instances));
    }

    #[test]
    fn las_refs_de_la_config_se_validan_con_la_misma_gramatica() {
        // Interpolación válida sobre una ruta declarada.
        let ok = validate_chain(&[
            origen(json!({})),
            con_config(
                destino(json!({}), json!({})),
                json!({ "url": "https://api.x.com/u/{$.origen.cliente.email}" }),
            ),
        ]);
        assert_sin_errores(&ok);

        // Interpolación sobre una ruta imposible.
        let imposible = validate_chain(&[
            origen(json!({})),
            con_config(
                destino(json!({}), json!({})),
                json!({ "headers": { "x": "Bearer {$.origen.token}" } }),
            ),
        ]);
        assert_eq!(
            campos(&imposible),
            vec!["blocks[1].config.headers.x"],
            "{}",
            mensajes(&imposible)
        );

        // Interpolación sin cerrar: error de sintaxis.
        let rota = validate_chain(&[
            origen(json!({})),
            con_config(
                destino(json!({}), json!({})),
                json!({ "url": "https://x/{$.origen.id" }),
            ),
        ]);
        assert_eq!(campos(&rota), vec!["blocks[1].config.url"]);

        // Ref no resuelta en la config (bloque inexistente).
        let ajena = validate_chain(&[
            origen(json!({})),
            con_config(
                destino(json!({}), json!({})),
                json!({ "url": "{$.auth.userId}" }),
            ),
        ]);
        assert_eq!(campos(&ajena), vec!["blocks[1].config.url"]);
        assert!(ajena[0].message.contains("auth"), "{}", mensajes(&ajena));
    }

    // ── compatibilidad de tipos campo a campo ──────────────────────────

    #[test]
    fn el_tipo_mapeado_debe_ser_compatible_con_el_input_schema_del_destino() {
        let estricto = json!({
            "type": "object",
            "properties": { "total": { "type": "integer" }, "nota": { "type": "string" } }
        });

        // integer → number es compatible (ensanchamiento).
        let ok = validate_chain(&[
            origen(json!({})),
            destino(
                json!({ "total": "$.origen.total" }),
                json!({ "type": "object", "properties": { "total": { "type": "number" } } }),
            ),
        ]);
        assert_sin_errores(&ok);

        // string → integer no lo es.
        let roto = validate_chain(&[
            origen(json!({})),
            destino(
                json!({ "total": "$.origen.cliente.email" }),
                json!({ "type": "object", "properties": { "total": { "type": "integer" } } }),
            ),
        ]);
        assert_eq!(
            campos(&roto),
            vec!["blocks[1].input.total"],
            "{}",
            mensajes(&roto)
        );
        assert!(
            roto[0].message.contains("string") && roto[0].message.contains("integer"),
            "el mensaje nombra ambos tipos: {}",
            mensajes(&roto)
        );

        // Sin tipo declarado en el destino no hay nada que comparar.
        let suelto = validate_chain(&[
            origen(json!({})),
            destino(
                json!({ "nota": "$.origen.total" }),
                json!({ "type": "object", "properties": { "nota": {} } }),
            ),
        ]);
        assert_sin_errores(&suelto);

        // Un tipo ausente en el origen tampoco bloquea.
        let sin_tipo = validate_chain(&[
            instancia(
                "origen",
                json!({}),
                json!({ "properties": { "campo": {} } }),
            ),
            destino(
                json!({ "n": "$.origen.campo" }),
                json!({ "type": "object", "properties": { "n": { "type": "integer" } } }),
            ),
        ]);
        assert_sin_errores(&sin_tipo);

        // El schema estricto de destino se usa tal cual (referencia de control).
        assert!(estricto["properties"]["total"]["type"] == json!("integer"));
    }

    #[test]
    fn un_valor_literal_en_el_mapeo_no_se_compara() {
        let instances = vec![destino(
            json!({ "total": 42, "flag": true, "nulo": null }),
            json!({ "type": "object", "properties": { "total": { "type": "integer" } } }),
        )];
        assert_sin_errores(&validate_chain(&instances));
    }

    // ── acumulación ────────────────────────────────────────────────────

    #[test]
    fn acumula_todos_los_errores_de_una_vez() {
        let mut rota = instancia("repe", json!({ "x": "$.nada.a" }), json!({}));
        rota.name = "origen".to_string();
        rota.id = "paso-origen".to_string();
        let instances = vec![
            origen(json!({ "a": "$.fantasma.id" })),
            rota,
            con_config(
                destino(json!({ "y": "$.origen.apellido" }), json!({})),
                json!({ "url": "{$.otro.bloque.id}" }),
            ),
        ];
        let issues = validate_chain(&instances);
        let fields = campos(&issues);
        for esperado in [
            "blocks[0].input.a",
            "blocks[1].name",
            "blocks[1].id",
            "blocks[1].input.x",
            "blocks[2].input.y",
            "blocks[2].config.url",
        ] {
            assert!(
                fields.contains(&esperado),
                "falta {esperado}: {}",
                mensajes(&issues)
            );
        }
        assert!(
            issues.len() >= 6,
            "se acumulan todos: {}",
            mensajes(&issues)
        );
    }
}
