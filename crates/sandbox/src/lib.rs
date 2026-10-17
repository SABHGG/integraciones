//! Sandbox: wrapper de rquickjs/quickjs-ng.
//!
//! Cada ejecución crea un runtime y contexto QuickJS frescos, aplica el límite
//! de memoria configurado, instala un interrupt handler que corta bucles
//! infinitos al vencer el timeout del puente y destruye todo al terminar.
//!
//! El contrato con el ejecutor es `run_block`: recibe el código JS ya compilado
//! (con `export default function(input, ctx)`) y un `serde_json::Value` de
//! entrada; devuelve la salida como `serde_json::Value` o un `SandboxError`.

mod convert;
mod host;
mod http;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rquickjs::promise::MaybePromise;
use rquickjs::{
    AsyncContext, AsyncRuntime, Ctx, Error, Function, Module, Value, async_with,
};
use serde_json::Value as Json;

use convert::{js_to_json, json_to_js};
use host::HostState;

/// Marca interna para transportar `HttpDenied` a través de una excepción JS.
pub(crate) const MARKER_HTTP_DENIED: &str = "SANDBOX_HTTP_DENIED:";
/// Marca interna para transportar errores de host a través de una excepción JS.
pub(crate) const MARKER_HOST_ERROR: &str = "SANDBOX_HOST_ERROR:";

/// Configuración de una ejecución de sandbox.
pub struct SandboxConfig {
    /// Timeout del puente en milisegundos. Corta bucles infinitos (interrupt)
    /// y acota el tiempo total de la ejecución (incluidas promises colgantes).
    pub timeout_ms: u64,
    /// Límite de memoria del isolate en bytes. `0` = sin límite.
    pub memory_limit_bytes: usize,
    /// Allowlist opcional de dominios para `ctx.http.request`. `None` = todo.
    pub allow_domains: Option<Vec<String>>,
    /// Resolutor de credenciales para `ctx.credential(name)`.
    pub credential_resolver: Arc<dyn Fn(String) -> Option<String> + Send + Sync>,
    /// Cliente HTTP reutilizable para `ctx.http.request`.
    pub http_client: reqwest::Client,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 5_000,
            memory_limit_bytes: 64 * 1024 * 1024,
            allow_domains: None,
            credential_resolver: Arc::new(|_name: String| None),
            http_client: reqwest::Client::new(),
        }
    }
}

/// Error estructurado de una ejecución de bloque.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// La ejecución excedió el timeout del puente.
    #[error("la ejecución excedió el timeout")]
    Timeout,
    /// El isolate agotó el límite de memoria configurado.
    #[error("el bloque agotó el límite de memoria")]
    MemoryLimit,
    /// El bloque lanzó una excepción JavaScript.
    #[error("error de JavaScript: {message}")]
    JsError {
        /// Mensaje de la excepción tal como la reportó QuickJS.
        message: String,
    },
    /// `ctx.http.request` intentó salir a un dominio fuera de la allowlist.
    #[error("dominio HTTP denegado: {domain}")]
    HttpDenied {
        /// Dominio que fue denegado.
        domain: String,
    },
    /// Una host function falló (red, URL inválida, deadline HTTP, etc.).
    #[error("error del host: {message}")]
    HostError {
        /// Detalle del fallo.
        message: String,
    },
}

/// Ejecuta un bloque JS compilado con `export default function(input, ctx)`.
///
/// Devuelve la salida del bloque como JSON o un [`SandboxError`]. Acepta tanto
/// retornos síncronos como promises (resueltas o rechazadas).
pub async fn run_block(
    code: &str,
    input: Json,
    cfg: SandboxConfig,
) -> Result<Json, SandboxError> {
    let timeout = Duration::from_millis(cfg.timeout_ms.max(1));

    let runtime = AsyncRuntime::new().map_err(|e| SandboxError::HostError {
        message: format!("creando runtime de QuickJS: {e}"),
    })?;

    if cfg.memory_limit_bytes > 0 {
        runtime.set_memory_limit(cfg.memory_limit_bytes).await;
    }

    // Interrupt handler: corta la ejecución al vencer el deadline incluso si el
    // bloque está en un bucle infinito. El flag distingue timeout de otros fallos.
    let timed_out = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + timeout;
    let interrupt_flag = timed_out.clone();
    runtime
        .set_interrupt_handler(Some(Box::new(move || {
            if Instant::now() >= deadline {
                interrupt_flag.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        })))
        .await;

    let context = AsyncContext::full(&runtime)
        .await
        .map_err(|e| SandboxError::HostError {
            message: format!("creando contexto de QuickJS: {e}"),
        })?;

    let state = Arc::new(HostState {
        allow_domains: cfg.allow_domains.clone(),
        credential_resolver: cfg.credential_resolver.clone(),
        http_client: cfg.http_client.clone(),
        http_timeout: timeout,
    });

    let run = async_with!(&context => |ctx| {
        let input_js = json_to_js(&ctx, &input).map_err(|e| describe_error(&ctx, e))?;
        let ctx_object = host::build_ctx_object(&ctx, state.clone())
            .map_err(|e| describe_error(&ctx, e))?;

        let module = Module::declare(ctx.clone(), "__block__", code)
            .map_err(|e| describe_error(&ctx, e))?;
        let (module, evaluation) = module.eval().map_err(|e| describe_error(&ctx, e))?;
        // El top-level del módulo puede ser asíncrono: hay que drenarlo.
        evaluation
            .into_future::<Value>()
            .await
            .map_err(|e| describe_error(&ctx, e))?;

        let handler: Function = module
            .get("default")
            .map_err(|e| describe_error(&ctx, e))?;
        match handler.call::<_, MaybePromise>((input_js, ctx_object)) {
            Ok(maybe_promise) => {
                let value = maybe_promise
                    .into_future::<Value>()
                    .await
                    .map_err(|e| describe_error(&ctx, e))?;
                js_to_json(&value).map_err(|e| describe_error(&ctx, e))
            }
            Err(e) => Err(describe_error(&ctx, e)),
        }
    });

    // Timeout externo: acota promises colgantes y HTTP lentos, donde el
    // interrupt del isolate no llega porque no hay código JS en ejecución.
    let outcome = tokio::time::timeout(timeout, run).await;

    // Destrucción explícita del isolate tras la ejecución.
    drop(context);
    drop(runtime);

    match outcome {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(message)) => Err(map_failure(&message, timed_out.load(Ordering::SeqCst))),
        Err(_elapsed) => Err(SandboxError::Timeout),
    }
}

/// Traduce el mensaje de error interno al [`SandboxError`] correspondiente.
fn map_failure(message: &str, timed_out: bool) -> SandboxError {
    if timed_out {
        return SandboxError::Timeout;
    }
    if message.to_ascii_lowercase().contains("out of memory") {
        return SandboxError::MemoryLimit;
    }
    if let Some(domain) = extract_marker(message, MARKER_HTTP_DENIED) {
        return SandboxError::HttpDenied { domain };
    }
    if let Some(detail) = extract_marker(message, MARKER_HOST_ERROR) {
        return SandboxError::HostError { message: detail };
    }
    SandboxError::JsError {
        message: message.to_string(),
    }
}

/// Extrae el texto que sigue a `marker` en un mensaje de error.
fn extract_marker(message: &str, marker: &str) -> Option<String> {
    message.find(marker).map(|index| {
        message[index + marker.len()..]
            .trim()
            .trim_matches(|c| c == '\'' || c == '"')
            .to_string()
    })
}

/// Convierte un error de rquickjs en un mensaje de error legible.
fn describe_error(ctx: &Ctx<'_>, error: Error) -> String {
    if !error.is_exception() {
        return error.to_string();
    }
    let caught = ctx.catch();
    if let Some(exception) = caught
        .as_object()
        .and_then(|object| rquickjs::Exception::from_object(object.clone()))
    {
        exception
            .message()
            .unwrap_or_else(|| "excepción JS sin mensaje".to_string())
    } else if let Some(text) = caught.as_string() {
        text.to_string()
            .unwrap_or_else(|_| "excepción JS sin mensaje".to_string())
    } else {
        "excepción JS sin mensaje".to_string()
    }
}