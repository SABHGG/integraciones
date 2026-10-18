//! Contrato ejecutor ↔ sandbox.
//!
//! El ejecutor no conoce rquickjs: recorre la cadena y delega la ejecución de
//! cada bloque en un [`BlockInvoker`]. El invoker de producción es
//! [`SandboxInvoker`], que crea un isolate fresco por bloque; los tests usan un
//! invoker falso para ejercitar la máquina sin JavaScript.

use std::future::Future;
use std::sync::Arc;

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
pub trait BlockInvoker {
    /// Ejecuta el bloque y devuelve su salida JSON.
    fn invoke(
        &self,
        request: InvokeRequest<'_>,
    ) -> impl Future<Output = Result<Json, SandboxError>>;
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
}