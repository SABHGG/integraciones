//! Contrato ejecutor ↔ sandbox.
//!
//! El ejecutor no conoce rquickjs: recorre la cadena y delega la ejecución de
//! cada bloque en un [`BlockInvoker`]. El invoker de producción es
//! [`SandboxInvoker`], que crea un isolate fresco por bloque; los tests usan un
//! invoker falso para ejercitar la máquina sin JavaScript.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use sandbox::{SandboxConfig, SandboxError, run_block};
use serde_json::Value as Json;

/// Petición de ejecución de un bloque.
#[derive(Debug, Clone)]
pub struct InvokeRequest<'a> {
    /// JS ya compilado con `export default function(input, ctx)`.
    pub code: &'a str,
    /// Entrada resuelta del bloque (mapeo de entrada ya resuelto contra el
    /// contexto de ejecución).
    pub input: Json,
    /// Config de instancia ya resuelta (interpolación aplicada). El sandbox de
    /// bloques CUSTOM_LOGIC no la consume: la usan los bloques built-in del
    /// host (tarea 4.6). Se resuelve aquí para que una ref rota en la config
    /// falle en el ejecutor y no en silencio.
    pub config: Json,
    /// Timeout del bloque en milisegundos.
    pub timeout_ms: u64,
}

/// Motor de ejecución de un bloque.
///
/// El futuro devuelto **no** es `Send`: `sandbox::run_block` usa el
/// `AsyncContext` full-async de rquickjs. El ejecutor se ejecuta en un hilo
/// current-thread (la decisión de workers/`LocalSet` para el binario de la API
/// se toma en la Fase 4).
/// Petición de ejecución de un **bloque built-in del host** (instancia sin
/// `code`: los bloques `http` de la biblioteca base, tarea 4.6). El ejecutor
/// resuelve refs e inputSchema igual que para CUSTOM_LOGIC; el comportamiento
/// vive en el invoker.
#[derive(Debug, Clone)]
pub struct BuiltInRequest<'a> {
    /// Config de instancia ya resuelta (interpolación `{$.ref}` aplicada).
    /// Los placeholders `{$credential:nombre}` quedan intactos: los resuelve
    /// el host justo antes de la llamada, sin que el secreto entre al IO.
    pub config: &'a Json,
    /// Timeout del bloque en milisegundos (también deadline de la petición).
    pub timeout_ms: u64,
}

pub trait BlockInvoker {
    /// Ejecuta el bloque y devuelve su salida JSON.
    fn invoke(
        &self,
        request: InvokeRequest<'_>,
    ) -> impl Future<Output = Result<Json, SandboxError>>;

    /// Ejecuta un bloque built-in `http` del host. El default falla: solo los
    /// invokers con salida a red lo implementan.
    fn invoke_built_in_http(
        &self,
        _request: BuiltInRequest<'_>,
    ) -> impl Future<Output = Result<Json, SandboxError>> {
        async {
            Err(SandboxError::HostError {
                message: "este invoker no soporta bloques built-in http".to_string(),
            })
        }
    }
}

/// Invoker de producción: un isolate QuickJS fresco por bloque.
pub struct SandboxInvoker {
    memory_limit_bytes: usize,
    allow_domains: Option<Vec<String>>,
    credential_resolver: Arc<dyn Fn(String) -> Option<String> + Send + Sync>,
    http_client: reqwest::Client,
}

impl Default for SandboxInvoker {
    fn default() -> Self {
        Self::new()
    }
}

impl SandboxInvoker {
    /// Invoker con los límites por defecto del sandbox.
    pub fn new() -> Self {
        let defaults = SandboxConfig::default();
        Self {
            memory_limit_bytes: defaults.memory_limit_bytes,
            allow_domains: None,
            credential_resolver: Arc::new(|_name: String| None),
            http_client: reqwest::Client::new(),
        }
    }

    /// Restringe los dominios que `ctx.http.request` puede alcanzar.
    pub fn with_allow_domains(mut self, domains: Vec<String>) -> Self {
        self.allow_domains = Some(domains);
        self
    }

    /// Define el resolutor de `ctx.credential(name)`. El valor del secreto
    /// vive solo en el host: nunca entra al contexto ni al IO persistido.
    pub fn with_credential_resolver(
        mut self,
        resolver: Arc<dyn Fn(String) -> Option<String> + Send + Sync>,
    ) -> Self {
        self.credential_resolver = resolver;
        self
    }

    /// Override del límite de memoria del isolate (bytes; `0` = sin límite).
    pub fn with_memory_limit_bytes(mut self, bytes: usize) -> Self {
        self.memory_limit_bytes = bytes;
        self
    }

    /// Cliente HTTP reutilizable para `ctx.http.request`.
    pub fn with_http_client(mut self, client: reqwest::Client) -> Self {
        self.http_client = client;
        self
    }
}

/// Placeholder de credencial en la config de un bloque built-in.
const CREDENTIAL_ANCHOR: &str = "{$credential:";

impl SandboxInvoker {
    /// Sustituye cada `{$credential:nombre}` de `text` por el secreto que
    /// resuelve el host. El secreto solo existe en memoria del host: jamás
    /// entra al contexto de ejecución ni al IO persistido.
    fn resolver_credenciales(&self, text: &str) -> Result<String, SandboxError> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find(CREDENTIAL_ANCHOR) {
            out.push_str(&rest[..open]);
            let despues = &rest[open + CREDENTIAL_ANCHOR.len()..];
            let Some(close) = despues.find('}') else {
                return Err(SandboxError::HostError {
                    message: "interpolación de credencial sin cerrar: falta '}'".to_string(),
                });
            };
            let name = &despues[..close];
            let Some(secret) = (self.credential_resolver)(name.to_string()) else {
                return Err(SandboxError::HostError {
                    message: format!(
                        "la credencial \"{name}\" no existe en la bóveda del workspace"
                    ),
                });
            };
            out.push_str(&secret);
            rest = &despues[close + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Resuelve placeholders de credenciales de forma recursiva en un valor de
    /// config (strings, arrays y objetos).
    fn resolver_credenciales_en_valor(&self, value: &Json) -> Result<Json, SandboxError> {
        match value {
            Json::String(text) => Ok(Json::String(self.resolver_credenciales(text)?)),
            Json::Array(items) => items
                .iter()
                .map(|item| self.resolver_credenciales_en_valor(item))
                .collect::<Result<Vec<_>, _>>()
                .map(Json::Array),
            Json::Object(map) => map
                .iter()
                .map(|(key, value)| {
                    Ok((key.clone(), self.resolver_credenciales_en_valor(value)?))
                })
                .collect::<Result<serde_json::Map<String, Json>, _>>()
                .map(Json::Object),
            otro => Ok(otro.clone()),
        }
    }

    /// Traduce un [`sandbox::http::HttpFailure`] al contrato de [`SandboxError`].
    fn traducir_http(fallo: sandbox::http::HttpFailure) -> SandboxError {
        match fallo {
            sandbox::http::HttpFailure::Denied(domain) => SandboxError::HttpDenied { domain },
            sandbox::http::HttpFailure::Timeout(url) => SandboxError::HostError {
                message: format!("http request timed out: {url}"),
            },
            sandbox::http::HttpFailure::Transport(detail) => SandboxError::HostError {
                message: format!("http request failed: {detail}"),
            },
        }
    }
}

impl BlockInvoker for SandboxInvoker {
    fn invoke(
        &self,
        request: InvokeRequest<'_>,
    ) -> impl Future<Output = Result<Json, SandboxError>> {
        let config = SandboxConfig {
            timeout_ms: request.timeout_ms,
            memory_limit_bytes: self.memory_limit_bytes,
            allow_domains: self.allow_domains.clone(),
            credential_resolver: self.credential_resolver.clone(),
            http_client: self.http_client.clone(),
        };
        let code = request.code.to_owned();
        async move { run_block(&code, request.input, config).await }
    }

    async fn invoke_built_in_http(
        &self,
        request: BuiltInRequest<'_>,
    ) -> Result<Json, SandboxError> {
        let config = request.config;

        let Some(url) = config.get("url").and_then(Json::as_str) else {
            return Err(SandboxError::HostError {
                message: "el bloque built-in http requiere config.url (string)".to_string(),
            });
        };
        let method = config
            .get("method")
            .and_then(Json::as_str)
            .unwrap_or("GET")
            .to_string();

        let mut headers = Vec::new();
        if let Some(objeto) = config.get("headers").and_then(Json::as_object) {
            for (nombre, valor) in objeto {
                let Some(texto) = valor.as_str() else {
                    return Err(SandboxError::HostError {
                        message: format!("el header '{nombre}' debe ser un string"),
                    });
                };
                headers.push((nombre.clone(), self.resolver_credenciales(texto)?));
            }
        }

        let body = match config.get("body") {
            None | Some(Json::Null) => None,
            Some(valor) => Some(self.resolver_credenciales_en_valor(valor)?),
        };

        sandbox::http::execute(
            &self.http_client,
            Duration::from_millis(request.timeout_ms),
            self.allow_domains.as_deref(),
            sandbox::http::HttpRequest {
                url: self.resolver_credenciales(url)?,
                method,
                headers,
                body,
            },
        )
        .await
        .map_err(Self::traducir_http)
    }
}