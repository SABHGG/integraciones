//! Bóveda: cifrado en reposo del secreto de las credenciales (task 4.7).
//!
//! Formato almacenado en `credentials.secret_encrypted`: `nonce(12) || AEAD`
//! con **AES-256-GCM** y nonce aleatorio por mensaje. La clave viene de la
//! variable `VAULT_KEY` (32 bytes en hex, 64 caracteres); si no está definida
//! se genera una clave efímera por proceso con un aviso ruidoso: sirve para
//! desarrollo y CI, pero los secretos no sobreviven un reinicio — producción
//! DEBE definir `VAULT_KEY`.
//!
//! El secreto en claro solo existe en memoria del host (bóveda precargada en
//! `invoke::cargar_boveda` y resolutor de `ctx.credential`): jamás se devuelve
//! por la API ni entra al IO entre bloques ni al store de ejecuciones.

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit};
use thiserror::Error;
use uuid::Uuid;

/// Longitud del nonce de GCM en bytes.
const NONCE_LEN: usize = 12;

/// Errores de la bóveda.
#[derive(Debug, Error)]
pub enum VaultError {
    /// Los bytes almacenados no tienen la forma `nonce || AEAD` o no
    /// descifran con la clave actual (rotación de clave sin migrar).
    #[error("los bytes almacenados no son un secreto válido de la bóveda")]
    Corrupt,
    /// El texto en claro no es UTF-8 válido.
    #[error("el secreto descifrado no es UTF-8 válido")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Clave de cifrado del proceso: `VAULT_KEY` (hex de 64) o una efímera
/// generada una sola vez por proceso (con aviso ruidoso).
pub fn key() -> &'static [u8; 32] {
    use std::sync::OnceLock;
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    KEY.get_or_init(|| {
        match std::env::var("VAULT_KEY") {
            Ok(hex_key) => {
                let mut bytes = [0u8; 32];
                if hex::decode_to_slice(&hex_key, &mut bytes).is_ok() {
                    return bytes;
                }
                eprintln!(
                    "VAULT_KEY no es un hex de 64 caracteres: se usa una clave efímera \
                     y los secretos guardados no serán legibles tras reiniciar"
                );
            }
            Err(_) => eprintln!(
                "VAULT_KEY no está definida: se usa una clave efímera por proceso \
                 (los secretos no sobreviven un reinicio). Define VAULT_KEY en producción"
            ),
        }
        // 32 bytes aleatorios sin dependencias extra: dos UUID v7 (16 bytes
        // de entropía cada uno).
        let mut clave = [0u8; 32];
        clave[..16].copy_from_slice(Uuid::now_v7().as_bytes());
        clave[16..].copy_from_slice(Uuid::now_v7().as_bytes());
        clave
    })
}

/// Cifra un secreto para la bóveda: `nonce(12) || AEAD`.
pub fn encrypt(plaintext: &str) -> Vec<u8> {
    let cipher = Aes256Gcm::new(key().into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    // Nonce aleatorio por mensaje: dos UUID v7 frescos dan 12 bytes con
    // entropía de sobra para el volumen de MVP1.
    let aleatorio = *Uuid::now_v7().as_bytes();
    nonce_bytes.copy_from_slice(&aleatorio[..NONCE_LEN]);

    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);
    let cifrado = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .expect("el cifrado AES-GCM con nonce de 12 bytes no falla");

    let mut almacenado = Vec::with_capacity(NONCE_LEN + cifrado.len());
    almacenado.extend_from_slice(&nonce_bytes);
    almacenado.extend_from_slice(&cifrado);
    almacenado
}

/// Descifra los bytes de la bóveda (`nonce(12) || AEAD`) al secreto en claro.
pub fn decrypt(stored: &[u8]) -> Result<String, VaultError> {
    if stored.len() <= NONCE_LEN {
        return Err(VaultError::Corrupt);
    }
    let (nonce_bytes, cifrado) = stored.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(key().into());
    let plaintext = cipher
        .decrypt(aes_gcm::Nonce::from_slice(nonce_bytes), cifrado)
        .map_err(|_| VaultError::Corrupt)?;
    String::from_utf8(plaintext).map_err(VaultError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cifra_y_descifra_un_roundtrip() {
        let almacenado = encrypt("sk_test_secreto_9f3b");
        assert_ne!(almacenado, b"sk_test_secreto_9f3b");
        assert!(!almacenado.windows(8).any(|w| w == b"secreto_9"));
        assert_eq!(decrypt(&almacenado).unwrap(), "sk_test_secreto_9f3b");
    }

    #[test]
    fn nonces_distintos_producen_cifrados_distintos() {
        assert_ne!(encrypt("mismo"), encrypt("mismo"));
    }

    #[test]
    fn bytes_ajenos_son_corrupt() {
        assert!(matches!(decrypt(b"corto"), Err(VaultError::Corrupt)));
        assert!(matches!(decrypt(&[0u8; 64]), Err(VaultError::Corrupt)));
    }
}
