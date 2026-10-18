//! Cursor opaco de paginación (design.md: "paginación por cursor opaco en
//! todos los listados").
//!
//! El cursor es base64url (sin relleno) de las partes separadas por `|`, de modo
//! que el cliente no puede interpretarlo ni construirlo a mano, pero el servidor
//! lo decodifica sin estado. Cada listado decide sus partes:
//! - bloques y puentes: `created_at (RFC 3339) | id`, keyset sobre
//!   `(created_at, id)`.
//! - versiones de bloque y de puente: `version`, keyset sobre `version`.
//!
//! Un cursor corrupto o con partes de más/menos es un 400 del handler.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Error de decodificación: el cursor no es opaco-válido.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CursorError {
    /// El texto no es base64url válido.
    #[error("el cursor no está en el formato esperado")]
    NotBase64,
    /// El cursor decodificó pero no tiene las partes esperadas.
    #[error("el cursor no tiene {expected} parte(s) separadas por \"|\"")]
    Malformed {
        /// Número de partes esperadas.
        expected: usize,
    },
    /// Alguna parte no tiene el tipo esperado (UUID, fecha, entero).
    #[error("el cursor tiene una parte inválida ({part}): {reason}")]
    InvalidPart {
        /// Nombre de la parte inválida.
        part: &'static str,
        /// Motivo concreto.
        reason: String,
    },
}

/// Máximo de caracteres aceptados para un cursor: descarta entradas absurdas
/// antes de decodificar.
const MAX_CURSOR_LEN: usize = 256;

/// Codifica las partes del cursor en base64url sin relleno.
pub fn encode(parts: &[&str]) -> String {
    URL_SAFE_NO_PAD.encode(parts.join("|"))
}

/// Decodifica el cursor y exige exactamente `expected` partes.
pub fn decode(raw: &str, expected: usize) -> Result<Vec<String>, CursorError> {
    if raw.len() > MAX_CURSOR_LEN {
        return Err(CursorError::NotBase64);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| CursorError::NotBase64)?;
    let text = String::from_utf8(bytes).map_err(|_| CursorError::NotBase64)?;
    let parts: Vec<String> = text.split('|').map(str::to_string).collect();
    if parts.len() != expected {
        return Err(CursorError::Malformed { expected });
    }
    Ok(parts)
}

/// Cursor de un listado keyset por `created_at | id` (bloques y puentes).
fn encode_row_cursor(created_at: DateTime<Utc>, id: Uuid) -> String {
    encode(&[&created_at.to_rfc3339(), &id.to_string()])
}

/// Decodifica un cursor keyset por `created_at | id`.
fn decode_row_cursor(raw: &str) -> Result<(DateTime<Utc>, Uuid), CursorError> {
    let parts = decode(raw, 2)?;
    let created_at = DateTime::parse_from_rfc3339(&parts[0])
        .map_err(|e| CursorError::InvalidPart {
            part: "created_at",
            reason: e.to_string(),
        })?
        .with_timezone(&Utc);
    let id = Uuid::parse_str(&parts[1]).map_err(|e| CursorError::InvalidPart {
        part: "id",
        reason: e.to_string(),
    })?;
    Ok((created_at, id))
}

/// Cursor del listado de bloques: `created_at | id`.
pub fn encode_block_cursor(created_at: DateTime<Utc>, id: Uuid) -> String {
    encode_row_cursor(created_at, id)
}

/// Decodifica el cursor del listado de bloques.
pub fn decode_block_cursor(raw: &str) -> Result<(DateTime<Utc>, Uuid), CursorError> {
    decode_row_cursor(raw)
}

/// Cursor del listado de puentes: `created_at | id`.
pub fn encode_bridge_cursor(created_at: DateTime<Utc>, id: Uuid) -> String {
    encode_row_cursor(created_at, id)
}

/// Decodifica el cursor del listado de puentes.
pub fn decode_bridge_cursor(raw: &str) -> Result<(DateTime<Utc>, Uuid), CursorError> {
    decode_row_cursor(raw)
}

/// Cursor del listado de versiones: `version` (UNIQUE (block_id, version) hace
/// el orden total).
pub fn encode_version_cursor(version: i32) -> String {
    encode(&[&version.to_string()])
}

/// Cursor del listado de ejecuciones: `created_at | id` (el UUID v7 ya es
/// ordenable, pero el cursor mantiene el mismo criterio que bloques/puentes).
pub fn encode_execution_cursor(created_at: DateTime<Utc>, id: Uuid) -> String {
    encode_row_cursor(created_at, id)
}

/// Decodifica el cursor del listado de ejecuciones.
pub fn decode_execution_cursor(raw: &str) -> Result<(DateTime<Utc>, Uuid), CursorError> {
    decode_row_cursor(raw)
}

/// Decodifica el cursor del listado de versiones.
pub fn decode_version_cursor(raw: &str) -> Result<i32, CursorError> {
    let parts = decode(raw, 1)?;
    parts[0].parse::<i32>().map_err(|e| CursorError::InvalidPart {
        part: "version",
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_cursor_de_bloques_va_y_vuelve() {
        let created_at = DateTime::parse_from_rfc3339("2026-01-02T03:04:05.123456Z")
            .unwrap()
            .with_timezone(&Utc);
        let id = Uuid::now_v7();

        let cursor = encode_block_cursor(created_at, id);
        // Opaco = el cliente no puede leer ni fabricar el keyset a mano: ni el
        // separador ni la fecha en claro sobreviven a la codificación.
        assert!(!cursor.contains('|'), "el cursor debe ser opaco: {cursor}");
        assert!(!cursor.contains(':'), "el cursor debe ser opaco: {cursor}");
        assert!(
            !cursor.starts_with("2026"),
            "el cursor no empieza por la fecha: {cursor}"
        );

        let (got_at, got_id) = decode_block_cursor(&cursor).expect("cursor válido");
        assert_eq!(got_id, id);
        assert_eq!(got_at, created_at);
    }

    #[test]
    fn el_cursor_de_versiones_va_y_vuelve() {
        let cursor = encode_version_cursor(42);
        assert_eq!(decode_version_cursor(&cursor).unwrap(), 42);
    }

    #[test]
    fn un_cursor_corrupto_es_un_error_sin_panico() {
        assert_eq!(decode("no-es-base64-@@", 2), Err(CursorError::NotBase64));
        // base64 válido de "3" pero con dos partes esperadas
        assert_eq!(
            decode(&encode(&["3"]), 2),
            Err(CursorError::Malformed { expected: 2 })
        );
        assert!(matches!(
            decode_block_cursor(&encode(&["ayer", "3"])),
            Err(CursorError::InvalidPart {
                part: "created_at",
                ..
            })
        ));
        assert!(matches!(
            decode_block_cursor(&encode(&["2026-01-02T03:04:05Z", "no-uuid"])),
            Err(CursorError::InvalidPart { part: "id", .. })
        ));
        assert!(matches!(
            decode_version_cursor(&encode(&["no-numero"])),
            Err(CursorError::InvalidPart {
                part: "version",
                ..
            })
        ));
    }

    #[test]
    fn un_cursor_demasiado_largo_se_rechaza_antes_de_decodificar() {
        let largo = "A".repeat(MAX_CURSOR_LEN + 1);
        assert_eq!(decode(&largo, 1), Err(CursorError::NotBase64));
    }

    #[test]
    fn cursors_de_distinto_listado_no_son_intercambiables() {
        let cursor = encode_block_cursor(Utc::now(), Uuid::now_v7());
        assert!(decode_version_cursor(&cursor).is_err());
    }
}