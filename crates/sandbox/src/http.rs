//! Lógica de `ctx.http.request`: parser de opciones, allowlist de dominios y
//! ejecución sobre `reqwest` con deadline propio por llamada.

use std::time::Duration;

use rquickjs::{Object, Value};
use serde_json::Value as Json;

/// Petición HTTP normalizada desde el objeto JS `{url, method, headers, body}`.
#[derive(Debug, Clone)]
pub(crate) struct HttpRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Json>,
}

/// Fallo de una petición HTTP originado en el host.
#[derive(Debug)]
pub(crate) enum HttpFailure {
    /// El dominio no está en la allowlist configurada.
    Denied(String),
    /// La petición excedió su deadline propio.
    Timeout(String),
    /// Error de red, URL o método inválidos.
    Transport(String),
}

impl HttpFailure {
    /// Convierte el fallo en el mensaje marcado que se lanza a JS.
    pub(crate) fn message(&self) -> String {
        match self {
            HttpFailure::Denied(domain) => format!("{}{domain}", super::MARKER_HTTP_DENIED),
            HttpFailure::Timeout(url) => format!(
                "{}http request timed out: {url}",
                super::MARKER_HOST_ERROR
            ),
            HttpFailure::Transport(detail) => {
                format!("{}http request failed: {detail}", super::MARKER_HOST_ERROR)
            }
        }
    }
}

/// Extrae y valida `{url, method, headers, body}` desde el objeto JS.
pub(crate) fn parse_request<'js>(options: &Object<'js>) -> Result<HttpRequest, String> {
    let url: String = options
        .get("url")
        .map_err(|_| "ctx.http.request: 'url' debe ser un string".to_string())?;
    if url.is_empty() {
        return Err("ctx.http.request: 'url' no puede estar vacío".to_string());
    }

    let method: Option<String> = options
        .get("method")
        .map_err(|_| "ctx.http.request: 'method' debe ser un string".to_string())?;

    let mut headers = Vec::new();
    if let Some(header_object) = options
        .get::<_, Option<Object<'js>>>("headers")
        .map_err(|_| "ctx.http.request: 'headers' debe ser un objeto".to_string())?
    {
        for entry in header_object.props::<String, Value>() {
            let (key, value) = entry.map_err(|e| format!("ctx.http.request: headers inválidos: {e}"))?;
            let value = value
                .as_string()
                .and_then(|s| s.to_string().ok())
                .ok_or_else(|| format!("ctx.http.request: header '{key}' no es un string"))?;
            headers.push((key, value));
        }
    }

    let body = match options.get::<_, Option<Value<'js>>>("body") {
        Ok(Some(value)) if !value.is_null() && !value.is_undefined() => {
            Some(super::convert::js_to_json(&value).map_err(|e| format!("ctx.http.request: body inválido: {e}"))?)
        }
        _ => None,
    };

    Ok(HttpRequest {
        url,
        method: method.unwrap_or_else(|| "GET".to_string()),
        headers,
        body,
    })
}

/// Indica si `host` está permitido por la allowlist.
///
/// `None` en la allowlist significa "permitir todo". Una entrada permite el
/// host exacto y sus subdominios (`example.com` cubre `api.example.com`).
pub(crate) fn domain_allowed(host: &str, allow: &[String]) -> bool {
    allow.iter().any(|entry| {
        let entry = entry.trim().trim_start_matches("*.").trim_end_matches('/');
        !entry.is_empty() && (host == entry || host.ends_with(&format!(".{entry}")))
    })
}

/// Ejecuta la petición con deadline propio y devuelve el body como JSON o texto.
pub(crate) async fn execute(
    client: &reqwest::Client,
    timeout: Duration,
    allow_domains: Option<&[String]>,
    request: HttpRequest,
) -> Result<Json, HttpFailure> {
    let url = reqwest::Url::parse(&request.url)
        .map_err(|e| HttpFailure::Transport(format!("url inválida '{}': {e}", request.url)))?;

    let host = url.host_str().unwrap_or_default().to_string();
    if let Some(allow) = allow_domains
        && !domain_allowed(&host, allow)
    {
        return Err(HttpFailure::Denied(host));
    }

    let method = reqwest::Method::from_bytes(request.method.to_uppercase().as_bytes())
        .map_err(|e| HttpFailure::Transport(format!("método inválido '{}': {e}", request.method)))?;

    let mut builder = client.request(method, url).timeout(timeout);
    for (key, value) in &request.headers {
        builder = builder.header(key.as_str(), value.as_str());
    }
    if let Some(body) = &request.body {
        match body {
            Json::String(text) => builder = builder.body(text.clone()),
            other => {
                builder = builder.header(reqwest::header::CONTENT_TYPE, "application/json");
                let serialized = serde_json::to_string(other)
                    .map_err(|e| HttpFailure::Transport(format!("body no serializable: {e}")))?;
                builder = builder.body(serialized);
            }
        }
    }

    let response = builder.send().await.map_err(|e| {
        if e.is_timeout() {
            HttpFailure::Timeout(request.url.clone())
        } else {
            HttpFailure::Transport(e.to_string())
        }
    })?;

    let is_json = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|content_type| content_type.contains("json"))
        .unwrap_or(false);

    let text = response
        .text()
        .await
        .map_err(|e| HttpFailure::Transport(format!("leyendo respuesta: {e}")))?;

    if is_json {
        match serde_json::from_str::<Json>(&text) {
            Ok(value) => Ok(value),
            Err(_) => Ok(Json::String(text)),
        }
    } else {
        Ok(Json::String(text))
    }
}
