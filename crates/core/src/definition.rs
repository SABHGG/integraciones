//! Modelo de la definición de un puente: el documento JSONB que
//! `bridge_versions.definition` guarda como **snapshot inmutable**.
//!
//! El snapshot lleva todo lo necesario para ejecutar sin consultar otras
//! tablas: bloques instanciados (con su JS compilado y sus schemas), el trigger
//! y el `inputSchema` del puente. Eso es lo que hace posible el replay futuro
//! con el IO persistido.
//!
//! Las claves del documento son `camelCase` (contrato JSON de la API):
//!
//! ```json
//! {
//!   "trigger": { "type": "http", "method": "POST", "path": "/x/demo" },
//!   "inputSchema": { "type": "object" },
//!   "blocks": [
//!     {
//!       "id": "paso-1",
//!       "name": "usuario",
//!       "code": "export default (input) => ({ ...input })",
//!       "input": { "userId": "$.trigger.body.userId" },
//!       "config": { "url": "https://api.x.com/users/{$.trigger.body.userId}" },
//!       "inputSchema": { "type": "object", "required": ["userId"] },
//!       "outputSchema": {},
//!       "timeoutMs": 5000
//!     }
//!   ]
//! }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Default de los documentos JSON opcionales de una instancia: objeto vacío
/// (equivale a "sin restricciones").
fn empty_object() -> Value {
    json!({})
}

/// Definición completa de un puente (snapshot de una versión publicada).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeDefinition {
    /// Disparador del puente, snapshot-eado en la versión.
    pub trigger: TriggerDefinition,
    /// Schema opcional que valida el body entrante (lo aplica el plano de
    /// invocación antes de ejecutar). `None` = sin validación de entrada.
    #[serde(default)]
    pub input_schema: Option<Value>,
    /// Cadena lineal de bloques, en orden de ejecución.
    #[serde(default)]
    pub blocks: Vec<BlockInstance>,
}

impl BridgeDefinition {
    /// Deserializa una definición desde el JSONB del store.
    pub fn from_json(value: &Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value.clone())
    }
}

/// Disparador del puente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerDefinition {
    /// Tipo de disparador (define la forma de `$.trigger`).
    #[serde(rename = "type")]
    pub trigger_type: TriggerType,
    /// Método HTTP aceptado (solo disparador `http`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Ruta de invocación (solo disparador `http`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Expresión cron de 5 campos (solo disparador `cron`), validada al
    /// publicar y congelada en el snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// Zona horaria IANA del schedule (solo disparador `cron`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// Tipos de disparador soportados por el contrato del trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerType {
    /// `$.trigger = {body, query}`.
    Http,
    /// `$.trigger = {scheduledAt, cronExpr, timezone}` (MVP2).
    Cron,
    /// `$.trigger = {topic, payload, receivedAt}` (MVP3).
    Event,
    /// Datos de prueba conformes al inputSchema del puente.
    Manual,
}

impl TriggerType {
    /// Valor textual tal como vive en `executions.trigger_type`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Cron => "cron",
            Self::Event => "event",
            Self::Manual => "manual",
        }
    }
}

/// Una instancia de bloque dentro de la definición.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInstance {
    /// Identificador de la instancia dentro de la definición
    /// (`execution_steps.block_instance_id`).
    pub id: String,
    /// Nombre con el que la salida del bloque queda registrada en el contexto
    /// de ejecución y `execution_steps.block_name`.
    pub name: String,
    /// Bloque de la biblioteca del que se instancia (informativo).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    /// Versión exacta del bloque (informativo: el snapshot ya lleva el código).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_version_id: Option<String>,
    /// JS ya compilado con `export default function(input, ctx)`.
    #[serde(default)]
    pub code: Option<String>,
    /// Condición declarativa (`domain::conditions`) que decide si la instancia
    /// se ejecuta: falso ⇒ paso `skipped` sin invocar el sandbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<Value>,
    /// Mapeo de entrada del bloque: valores literales o referencias `$.`.
    #[serde(default = "empty_object")]
    pub input: Value,
    /// Configuración de instancia: strings con interpolación `{$.ref}`.
    #[serde(default = "empty_object")]
    pub config: Value,
    /// Schema que valida la entrada resuelta antes de invocar el sandbox.
    #[serde(default = "empty_object")]
    pub input_schema: Value,
    /// Schema de salida del bloque (verificación estática al publicar).
    #[serde(default = "empty_object")]
    pub output_schema: Value,
    /// Override del timeout del puente en ms para este bloque.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserializa_una_definicion_completa_con_camel_case() {
        let doc = json!({
            "trigger": { "type": "http", "method": "POST", "path": "/x/demo" },
            "inputSchema": { "type": "object" },
            "blocks": [{
                "id": "paso-1",
                "name": "usuario",
                "code": "export default (input) => ({ ...input })",
                "input": { "userId": "$.trigger.body.userId" },
                "config": { "url": "https://api.x.com/users/{$.trigger.body.userId}" },
                "inputSchema": { "type": "object", "required": ["userId"] },
                "outputSchema": {},
                "timeoutMs": 2500
            }]
        });
        let def = BridgeDefinition::from_json(&doc).expect("definición válida");
        assert_eq!(def.trigger.trigger_type, TriggerType::Http);
        assert_eq!(def.trigger.method.as_deref(), Some("POST"));
        assert_eq!(def.blocks.len(), 1);
        let block = &def.blocks[0];
        assert_eq!(block.id, "paso-1");
        assert_eq!(block.name, "usuario");
        assert_eq!(block.timeout_ms, Some(2500));
        assert_eq!(block.input["userId"], json!("$.trigger.body.userId"));
        assert!(block.code.as_deref().unwrap().contains("export default"));
    }

    #[test]
    fn aplica_defaults_de_campos_opcionales() {
        let doc = json!({
            "trigger": { "type": "manual" },
            "blocks": [{ "id": "a", "name": "a" }]
        });
        let def = BridgeDefinition::from_json(&doc).expect("definición mínima");
        assert!(def.input_schema.is_none());
        assert_eq!(def.blocks[0].input, json!({}));
        assert_eq!(def.blocks[0].input_schema, json!({}));
        assert!(def.blocks[0].code.is_none());
        assert!(def.blocks[0].timeout_ms.is_none());
    }

    #[test]
    fn el_tipo_de_trigger_mapea_al_valor_de_la_columna() {
        assert_eq!(TriggerType::Http.as_str(), "http");
        assert_eq!(TriggerType::Cron.as_str(), "cron");
        assert_eq!(TriggerType::Event.as_str(), "event");
        assert_eq!(TriggerType::Manual.as_str(), "manual");
    }

    #[test]
    fn rechaza_un_trigger_sin_tipo_conocido() {
        let doc = json!({ "trigger": { "type": "pigeon" }, "blocks": [] });
        assert!(BridgeDefinition::from_json(&doc).is_err());
    }
}
