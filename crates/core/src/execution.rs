//! Tipos del modelo de ejecución compartidos por el ejecutor, el store y la
//! API: estados, códigos de error estables y el error estructurado que se
//! persiste en `executions.error` / `execution_steps.error`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Códigos de error **estables** del contrato de ejecución
/// (design.md → "Semántica de errores y respuesta").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// La entrada resuelta de un bloque no cumple su `inputSchema`.
    ValidationFailed,
    /// El bloque falló al ejecutarse (throw, memoria, host, …).
    BlockFailed,
    /// El bloque excedió el timeout del puente.
    Timeout,
    /// Una referencia `$.` no resolvió contra el contexto de ejecución.
    RefUnresolved,
}

impl ErrorCode {
    /// Representación textual estable (la que se guarda en la BD y se expone).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ValidationFailed => "VALIDATION_FAILED",
            Self::BlockFailed => "BLOCK_FAILED",
            Self::Timeout => "TIMEOUT",
            Self::RefUnresolved => "REF_UNRESOLVED",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error estructurado de una ejecución o de un paso.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// Código estable del contrato.
    pub code: ErrorCode,
    /// Mensaje legible, quirúrgico (incluye la ruta o el campo culpable).
    pub message: String,
    /// Bloque donde ocurrió el fallo (nombre de la instancia), si aplica.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    /// Detalles estructurados específicos del código.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl ErrorPayload {
    /// Construye un error de ejecución sin detalles estructurados.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            block: None,
            details: None,
        }
    }

    /// Añade el bloque culpable.
    pub fn with_block(mut self, block: impl Into<String>) -> Self {
        self.block = Some(block.into());
        self
    }

    /// Añade detalles estructurados.
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl std::fmt::Display for ErrorPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.block {
            Some(block) => write!(f, "{} en bloque {block}: {}", self.code, self.message),
            None => write!(f, "{}: {}", self.code, self.message),
        }
    }
}

/// Estado de una ejecución (`executions.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// En cola: la fila es la unidad de trabajo de los workers.
    Pending,
    /// Reclamada por un worker.
    Running,
    /// Terminó sin error: el trace está completo.
    Succeeded,
    /// Terminó con error en el primer fallo (fail-fast).
    Failed,
}

impl ExecutionStatus {
    /// Representación textual tal como vive en la columna `status`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    /// Interpreta el valor de la columna `status`.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

impl std::fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Estado de un paso (`execution_steps.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// El bloque terminó y dejó salida.
    Succeeded,
    /// El bloque es el punto exacto del fallo (o falló antes de invocarse).
    Failed,
    /// El bloque no se ejecutó por enrutado condicional (`when` falso o ruta
    /// de `switch` no tomada): sin input ejecutado, salida ni duración.
    Skipped,
}

impl StepStatus {
    /// Representación textual tal como vive en la columna `status`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

impl std::fmt::Display for StepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn los_codigos_de_error_son_estables_en_json() {
        let payload = ErrorPayload::new(ErrorCode::ValidationFailed, "campo id inválido")
            .with_block("usuario")
            .with_details(json!({ "field": "id" }));
        let serialized = serde_json::to_value(&payload).expect("serializa");
        assert_eq!(serialized["code"], json!("VALIDATION_FAILED"));
        assert_eq!(serialized["block"], json!("usuario"));
        assert_eq!(serialized["details"], json!({ "field": "id" }));
    }

    #[test]
    fn los_campos_opcionales_del_error_no_se_serializan_vacios() {
        let value = serde_json::to_value(ErrorPayload::new(ErrorCode::Timeout, "5s")).unwrap();
        let obj = value.as_object().expect("objeto");
        assert!(!obj.contains_key("block"));
        assert!(!obj.contains_key("details"));
        assert_eq!(value["code"], json!("TIMEOUT"));
    }

    #[test]
    fn el_error_serializado_se_puede_releer_del_store() {
        let original = ErrorPayload::new(ErrorCode::BlockFailed, "boom")
            .with_block("paso-2")
            .with_details(json!({ "reason": "JS_ERROR" }));
        let roundtrip: ErrorPayload =
            serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
        assert_eq!(roundtrip, original);
    }

    #[test]
    fn los_estados_mapean_a_los_valores_de_la_columna() {
        for (status, raw) in [
            (ExecutionStatus::Pending, "pending"),
            (ExecutionStatus::Running, "running"),
            (ExecutionStatus::Succeeded, "succeeded"),
            (ExecutionStatus::Failed, "failed"),
        ] {
            assert_eq!(status.as_str(), raw);
            assert_eq!(ExecutionStatus::parse(raw), Some(status));
        }
        assert_eq!(ExecutionStatus::parse("cancelled"), None);
        assert_eq!(StepStatus::Succeeded.as_str(), "succeeded");
        assert_eq!(StepStatus::Failed.as_str(), "failed");
        assert_eq!(StepStatus::Skipped.as_str(), "skipped");
    }

    #[test]
    fn el_display_del_error_nombra_el_bloque() {
        let payload = ErrorPayload::new(ErrorCode::RefUnresolved, "falta userId").with_block("b2");
        let text = payload.to_string();
        assert!(text.contains("REF_UNRESOLVED"), "{text}");
        assert!(text.contains("b2"), "{text}");
    }
}
