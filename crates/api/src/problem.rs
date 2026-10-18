//! Errores Problem Details (RFC 9457, application/problem+json) para el plano
//! de administración. Los fallos de transporte del plano de invocación también
//! los usan (404/405/413/415).

use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};

/// Un problema RFC 9457. `extensions` se aplana en el objeto raíz
/// (la extensión de miembros es parte del estándar).
#[derive(Debug, Clone)]
pub struct Problem {
    pub status: StatusCode,
    pub type_slug: String, // p.ej. "unauthorized", "not-found"
    pub title: String,
    pub detail: Option<String>,
    pub instance: Option<String>,
    pub extensions: Map<String, Value>,
}

impl Problem {
    pub fn new(status: StatusCode, type_slug: &str, title: impl Into<String>) -> Self {
        Self {
            status,
            type_slug: type_slug.to_string(),
            title: title.into(),
            detail: None,
            instance: None,
            extensions: Map::new(),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Se usa desde la Fase 4 (idempotencia, validación de entrada).
    pub fn with_extension(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }

    pub fn to_json(&self, instance: Option<&str>) -> Value {
        let mut body = json!({
            "type": format!("https://integraciones.dev/problems/{}", self.type_slug),
            "title": self.title,
            "status": self.status.as_u16(),
        });
        let obj = body.as_object_mut().expect("json literal es objeto");
        if let Some(detail) = &self.detail {
            obj.insert("detail".into(), Value::String(detail.clone()));
        }
        if let Some(instance) = instance.or(self.instance.as_deref()) {
            obj.insert("instance".into(), Value::String(instance.to_string()));
        }
        for (k, v) in &self.extensions {
            obj.insert(k.clone(), v.clone());
        }
        body
    }

    pub fn into_response_with(self, instance: Option<&str>) -> Response {
        Response::builder()
            .status(self.status)
            .header(header::CONTENT_TYPE, "application/problem+json")
            .body(Body::from(self.to_json(instance).to_string()))
            .expect("Problem: builder con status válido no puede fallar")
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        self.into_response_with(None)
    }
}

// ── Constructores de los problemas comunes ─────────────────────────────

pub fn unauthorized(detail: impl Into<String>) -> Problem {
    Problem::new(StatusCode::UNAUTHORIZED, "unauthorized", "Unauthorized").with_detail(detail)
}

pub fn internal(detail: impl Into<String>) -> Problem {
    Problem::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "Internal Server Error",
    )
    .with_detail(detail)
}

pub fn forbidden(detail: impl Into<String>) -> Problem {
    Problem::new(StatusCode::FORBIDDEN, "forbidden", "Forbidden").with_detail(detail)
}

pub fn not_found(detail: impl Into<String>) -> Problem {
    Problem::new(StatusCode::NOT_FOUND, "not-found", "Not Found").with_detail(detail)
}

pub fn conflict(detail: impl Into<String>) -> Problem {
    Problem::new(StatusCode::CONFLICT, "conflict", "Conflict").with_detail(detail)
}

/// Entrada inválida a nivel de transporte o de query string (400).
pub fn bad_request(detail: impl Into<String>) -> Problem {
    Problem::new(StatusCode::BAD_REQUEST, "bad-request", "Bad Request").with_detail(detail)
}

/// Cuerpo bien formado pero que no cumple el contrato (JSON Schema fuera del
/// subconjunto, código TypeScript que no compila, `ui` inválido, …).
pub fn unprocessable_entity(detail: impl Into<String>) -> Problem {
    Problem::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "unprocessable-entity",
        "Unprocessable Entity",
    )
    .with_detail(detail)
}

pub fn not_implemented(detail: impl Into<String>) -> Problem {
    Problem::new(
        StatusCode::NOT_IMPLEMENTED,
        "not-implemented",
        "Not Implemented",
    )
    .with_detail(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header;

    #[test]
    fn to_json_incluye_campos_basicos_y_extensiones_aplanadas() {
        let p = Problem::new(StatusCode::CONFLICT, "conflict", "Conflict")
            .with_detail("bloque referenciado")
            .with_extension("bridgeSlug", serde_json::json!("demo"));
        let body = p.to_json(Some("/api/v1/blocks/x"));
        assert_eq!(body["status"], 409);
        assert_eq!(body["title"], "Conflict");
        assert_eq!(body["detail"], "bloque referenciado");
        assert_eq!(body["bridgeSlug"], "demo");
        assert_eq!(body["instance"], "/api/v1/blocks/x");
        assert!(
            body["type"]
                .as_str()
                .unwrap()
                .ends_with("/problems/conflict")
        );
    }

    #[test]
    fn into_response_usa_problem_json_y_status_correctos() {
        let resp = unauthorized("sin header").into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );
    }

    #[test]
    fn campos_opcionales_no_aparecen_si_no_se_definen() {
        let body = not_found("no existe").to_json(None);
        assert!(body.get("instance").is_none());
        assert!(body.get("detail").is_some());
    }

    #[test]
    fn constructores_de_conflicto_prohibido_y_validacion() {
        assert_eq!(conflict("slug duplicado").status, StatusCode::CONFLICT);
        assert_eq!(forbidden("sin workspace").status, StatusCode::FORBIDDEN);
        assert_eq!(
            bad_request("cursor inválido").status,
            StatusCode::BAD_REQUEST
        );
        let unprocessable = unprocessable_entity("código no compila");
        assert_eq!(unprocessable.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(unprocessable.type_slug, "unprocessable-entity");
        assert!(
            unprocessable.to_json(None)["type"]
                .as_str()
                .unwrap()
                .ends_with("/problems/unprocessable-entity")
        );
    }
}
