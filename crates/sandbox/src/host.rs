//! Estado del host y construcción del objeto `ctx` expuesto a los bloques.
//!
//! `ctx` es la única frontera de acceso al host: `ctx.log`, `ctx.crypto`,
//! `ctx.json`, `ctx.credential` y `ctx.http.request`.

use std::sync::Arc;
use std::time::Duration;

use rquickjs::function::Async;
use rquickjs::{Ctx, Error, Function, Object, Result, Value};

use crate::convert::{js_to_json, json_to_js};
use crate::http;

/// Estado compartido por las host functions de una ejecución.
pub(crate) struct HostState {
    /// Allowlist opcional de dominios para `ctx.http.request`.
    pub allow_domains: Option<Vec<String>>,
    /// Resolutor de credenciales (`ctx.credential`).
    pub credential_resolver: Arc<dyn Fn(String) -> Option<String> + Send + Sync>,
    /// Cliente HTTP reutilizable.
    pub http_client: reqwest::Client,
    /// Deadline propio por llamada HTTP.
    pub http_timeout: Duration,
}

/// Construye un error de host con la marca que el orquestador sabe decodificar.
fn host_error(message: String) -> Error {
    Error::new_from_js_message("sandbox", "host", message)
}

/// Emite un log estructurado del bloque hacia `tracing`.
fn log_message(level: &str, message: &str) {
    match level.to_ascii_lowercase().as_str() {
        "error" => tracing::error!(target: "sandbox.block", "{message}"),
        "warn" | "warning" => tracing::warn!(target: "sandbox.block", "{message}"),
        "debug" => tracing::debug!(target: "sandbox.block", "{message}"),
        "trace" => tracing::trace!(target: "sandbox.block", "{message}"),
        _ => tracing::info!(target: "sandbox.block", "{message}"),
    }
}

/// Construye el objeto `ctx` con las capacidades del host.
pub(crate) fn build_ctx_object<'js>(ctx: &Ctx<'js>, state: Arc<HostState>) -> Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;

    // ctx.log(level, message)
    object.set(
        "log",
        Function::new(ctx.clone(), move |level: String, message: String| {
            log_message(&level, &message);
        }),
    )?;

    // ctx.crypto
    let crypto = Object::new(ctx.clone())?;
    crypto.set(
        "sha256",
        Function::new(ctx.clone(), |data: String| -> String {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(data.as_bytes());
            hex::encode(hasher.finalize())
        }),
    )?;
    crypto.set(
        "hmacSha256",
        Function::new(ctx.clone(), |key: String, data: String| -> Result<String> {
            use hmac::{Hmac, Mac};
            use sha2::Sha256;
            let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
                .map_err(|e| host_error(format!("hmacSha256: clave inválida: {e}")))?;
            mac.update(data.as_bytes());
            Ok(hex::encode(mac.finalize().into_bytes()))
        }),
    )?;
    crypto.set(
        "base64Encode",
        Function::new(ctx.clone(), |data: String| -> String {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(data.as_bytes())
        }),
    )?;
    crypto.set(
        "base64Decode",
        Function::new(ctx.clone(), |data: String| -> Result<String> {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.as_bytes())
                .map_err(|e| host_error(format!("base64Decode: {e}")))?;
            std::string::String::from_utf8(bytes)
                .map_err(|e| host_error(format!("base64Decode: {e}")))
        }),
    )?;
    object.set("crypto", crypto)?;

    // ctx.json
    let json_object = Object::new(ctx.clone())?;
    let parse_ctx = ctx.clone();
    json_object.set(
        "parse",
        Function::new(ctx.clone(), move |text: String| -> Result<Value<'js>> {
            let parsed: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| host_error(format!("json.parse: {e}")))?;
            json_to_js(&parse_ctx, &parsed)
        }),
    )?;
    json_object.set(
        "stringify",
        Function::new(ctx.clone(), |value: Value<'js>| -> Result<String> {
            let json = js_to_json(&value)?;
            serde_json::to_string(&json).map_err(|e| host_error(format!("json.stringify: {e}")))
        }),
    )?;
    object.set("json", json_object)?;

    // ctx.credential(name)
    let resolver = state.credential_resolver.clone();
    object.set(
        "credential",
        Function::new(ctx.clone(), move |name: String| -> Option<String> {
            resolver(name)
        }),
    )?;

    // ctx.http.request({url, method, headers, body})
    let http_object = Object::new(ctx.clone())?;
    let http_state = state.clone();
    let http_ctx = ctx.clone();
    http_object.set(
        "request",
        Function::new(
            ctx.clone(),
            Async(move |options: Object<'js>| {
                let state = http_state.clone();
                let ctx = http_ctx.clone();
                async move {
                    let request = http::parse_request(&options).map_err(host_error)?;
                    let value = http::execute(
                        &state.http_client,
                        state.http_timeout,
                        state.allow_domains.as_deref(),
                        request,
                    )
                    .await
                    .map_err(|failure| host_error(failure.message()))?;
                    json_to_js(&ctx, &value)
                }
            }),
        ),
    )?;
    object.set("http", http_object)?;

    Ok(object)
}
