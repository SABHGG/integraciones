//! Auth del plano de administración con dos identidades por `Bearer`:
//!
//! - **token de sesión de usuario** (`user_sessions`, MVP4): resuelve al
//!   usuario y su rol en el workspace vía `workspace_members`;
//! - **API key de workspace** (MVP1, compatibilidad): actúa como `owner`.
//!
//! Ni los tokens ni las keys se guardan en crudo: se compara SHA-256.

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::problem;
use crate::state::AppState;

/// Rol del miembro en el workspace (`workspace_members.role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Editor,
    Runner,
    Viewer,
}

impl Role {
    /// El rol desde la BD (`text` con CHECK) — inválido es error de servidor.
    pub fn from_db(raw: &str) -> Result<Role, String> {
        match raw {
            "owner" => Ok(Role::Owner),
            "editor" => Ok(Role::Editor),
            "runner" => Ok(Role::Runner),
            "viewer" => Ok(Role::Viewer),
            otro => Err(format!("rol desconocido en workspace_members: {otro}")),
        }
    }

    /// Etiqueta canónica para mensajes de error (403).
    pub fn etiqueta(&self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Editor => "editor",
            Role::Runner => "runner",
            Role::Viewer => "viewer",
        }
    }

    /// ¿Cubre este rol lo que exige `requerido`? La jerarquía es lineal:
    /// owner ⊃ editor ⊃ runner ⊃ viewer.
    pub fn cubre(self, requerido: Role) -> bool {
        self.rango() >= requerido.rango()
    }

    /// Rango de jerarquía: a más alto, más cubre.
    fn rango(self) -> u8 {
        match self {
            Role::Viewer => 0,
            Role::Runner => 1,
            Role::Editor => 2,
            Role::Owner => 3,
        }
    }
}

/// Acciones de EJECUCIÓN (runner): disparar, ensayar, re-ejecutar, publicar
/// eventos. Cualquier otra escritura es administración de recursos (editor).
fn es_accion_ejecutar(segmentos: &[&str]) -> bool {
    matches!(
        segmentos.last(),
        Some(&"run") | Some(&"dry-run") | Some(&"replay")
    )
}

/// Matriz de permisos del plano admin (design D4), declarativa y en UN solo
/// lugar: (método, ruta) → rol mínimo. `GET` es lectura (viewer); ejecutar
/// (run/dry-run/replay y publicar eventos) es runner; el resto de escrituras
/// administran recursos (editor); la membresía es del owner. `whoami` es de
/// cualquier identidad. Las rutas `/auth/*` nunca llegan aquí (router público).
pub fn rol_requerido(metodo: &str, ruta: &str) -> Role {
    let ruta = ruta.trim_start_matches("/api/v1");
    let segmentos: Vec<&str> = ruta.split('/').filter(|s| !s.is_empty()).collect();
    match segmentos.first().copied() {
        Some("whoami") => Role::Viewer,
        Some("members") => Role::Owner,
        Some("events") => Role::Runner,
        _ if metodo == "GET" => Role::Viewer,
        _ if matches!(segmentos.first(), Some(&"bridges") | Some(&"executions"))
            && es_accion_ejecutar(&segmentos) =>
        {
            Role::Runner
        }
        _ => Role::Editor,
    }
}

/// Quién está detrás del `Bearer` (auditoría y `whoami`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Principal {
    /// Token de sesión de usuario (MVP4).
    User { user_id: Uuid, email: String },
    /// API key de workspace (MVP1): automatización, rol `owner`.
    ApiKey,
}

/// Identidad autenticada, disponible para los handlers vía
/// `Extension<AuthedKey>`.
///
/// `workspace_id` es opcional porque la FK de `api_keys` lo es y las sesiones
/// multi-workspace nacen sin workspace fijado (exigen `X-Workspace`).
#[derive(Debug, Clone, Serialize)]
pub struct AuthedKey {
    /// Id de la fila origen (`user_sessions` o `api_keys`).
    pub id: Uuid,
    /// Email (usuario) o nombre legible (API key).
    pub name: String,
    /// Workspace al que está vinculada la identidad, si lo tiene.
    pub workspace_id: Option<Uuid>,
    /// Rol efectivo en ese workspace (keys = `owner`).
    pub role: Role,
    /// Quién es (auditoría).
    pub principal: Principal,
}

impl AuthedKey {
    /// Workspace obligatorio para los handlers de recursos (bloques, puentes,
    /// credenciales): sin vínculo se responde 403 Problem Details.
    // `Problem` es grande (~150 bytes) pero solo se construye en la ruta de
    // error; mismo criterio que el middleware de este módulo.
    #[allow(clippy::result_large_err)]
    pub fn workspace(&self) -> Result<Uuid, problem::Problem> {
        self.workspace_id.ok_or_else(|| {
            problem::forbidden(
                "la identidad no está vinculada a un workspace; \
                 vincula la key o entra con `X-Workspace` para usar los recursos",
            )
        })
    }

    /// Email si el principal es usuario (auditoría del disparador).
    pub fn email(&self) -> Option<&str> {
        match &self.principal {
            Principal::User { email, .. } => Some(email),
            Principal::ApiKey => None,
        }
    }
}

pub fn hash_key(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

/// Sesión de usuario resuelta del token (antes de fijar workspace).
struct Sesion {
    id: Uuid,
    user_id: Uuid,
    email: String,
    workspace_id: Option<Uuid>,
}

/// Middleware del plano `/api/v1`: exige `Bearer` válido (token de usuario o
/// API key) y anota el principal con su rol. Las sesiones sin workspace fijado
/// exigen el header `X-Workspace` (slug o id) y resuelven el rol por petición.
// axum idiomático: el middleware devuelve Response tanto en éxito como en error;
// Response supera los 128 bytes, así que silenciamos el lint en lugar de boxear.
#[allow(clippy::result_large_err)]
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    let token = header
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty());

    let token = match token {
        Some(t) => t,
        None => {
            return Err(problem::unauthorized(
                "Falta el header Authorization: Bearer <token o api key>",
            )
            .into_response());
        }
    };

    let hash = hash_key(token);

    // 1) ¿Sesión de usuario? (los tokens nacen con el mismo formato que las
    //    keys: el hash decide — una colisión entre tablas es imposible a la
    //    práctica y ambas tablas se consultan en orden fijo).
    let sesion = sqlx::query_as::<_, (Uuid, Uuid, String, Option<Uuid>)>(
        "SELECT s.id, s.user_id, u.email, s.workspace_id \
         FROM user_sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now()",
    )
    .bind(&hash)
    .fetch_optional(&state.pool)
    .await;

    let sesion = match sesion {
        Ok(Some((id, user_id, email, workspace_id))) => Some(Sesion {
            id,
            user_id,
            email,
            workspace_id,
        }),
        Ok(None) => None,
        Err(e) => {
            tracing::error!(error = %e, "fallo al validar la sesión");
            return Err(problem::internal("fallo al validar la sesión").into_response());
        }
    };

    let autenticada = if let Some(sesion) = sesion {
        // best-effort: actualizar último uso sin bloquear la petición si falla
        let _ = sqlx::query("UPDATE user_sessions SET last_used_at = now() WHERE id = $1")
            .bind(sesion.id)
            .execute(&state.pool)
            .await;

        // Workspace fijado en la sesión o exigido por header (multi-workspace).
        let (workspace_id, role) = match sesion.workspace_id {
            Some(workspace_id) => {
                let rol = rol_en_workspace(&state, sesion.user_id, workspace_id).await?;
                (Some(workspace_id), rol)
            }
            None => {
                let pedido = req
                    .headers()
                    .get("x-workspace")
                    .and_then(|v| v.to_str().ok())
                    .map(str::trim)
                    .filter(|v| !v.is_empty());
                match pedido {
                    None => {
                        return Err(problem::unauthorized(
                            "la sesión no tiene workspace fijado: indica X-Workspace \
                             (o vuelve a entrar eligiendo workspace)",
                        )
                        .into_response());
                    }
                    Some(crudo) => resolver_por_header(&state, sesion.user_id, crudo).await?,
                }
            }
        };

        AuthedKey {
            id: sesion.id,
            name: sesion.email.clone(),
            workspace_id,
            role,
            principal: Principal::User {
                user_id: sesion.user_id,
                email: sesion.email,
            },
        }
    } else {
        // 2) ¿API key? (compatibilidad MVP1: automatización como owner).
        let row = sqlx::query_as::<_, (Uuid, String, Option<Uuid>)>(
            "SELECT id, name, workspace_id FROM api_keys \
             WHERE key_hash = $1 AND revoked_at IS NULL",
        )
        .bind(&hash)
        .fetch_optional(&state.pool)
        .await;

        match row {
            Ok(Some(row)) => {
                let _ = sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE id = $1")
                    .bind(row.0)
                    .execute(&state.pool)
                    .await;
                AuthedKey {
                    id: row.0,
                    name: row.1,
                    workspace_id: row.2,
                    role: Role::Owner,
                    principal: Principal::ApiKey,
                }
            }
            Ok(None) => {
                return Err(
                    problem::unauthorized("token o API key inválida o revocada").into_response()
                );
            }
            Err(e) => {
                tracing::error!(error = %e, "fallo al validar api key");
                return Err(problem::internal("fallo al validar la api key").into_response());
            }
        }
    };

    // Matriz de roles (design D4): denegación 403 con el rol requerido,
    // ANTES de que el handler toque nada.
    let requerido = rol_requerido(req.method().as_str(), req.uri().path());
    if !autenticada.role.cubre(requerido) {
        return Err(problem::forbidden(format!(
            "esta acción requiere rol {} y tu identidad tiene {}",
            requerido.etiqueta(),
            autenticada.role.etiqueta()
        ))
        .into_response());
    }

    req.extensions_mut().insert(autenticada);
    Ok(next.run(req).await)
}

/// Rol vigente del usuario en un workspace (por petición: un cambio de rol es
/// efectivo al instante). Sin membresía → la sesión ya no sirve ahí (401/403).
#[allow(clippy::result_large_err)]
async fn rol_en_workspace(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Role, Response> {
    let fila: Result<Option<String>, _> = sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE user_id = $1 AND workspace_id = $2",
    )
    .bind(user_id)
    .bind(workspace_id)
    .fetch_optional(&state.pool)
    .await;
    match fila {
        Ok(Some(rol)) => Role::from_db(&rol).map_err(|e| problem::internal(e).into_response()),
        Ok(None) => {
            Err(problem::unauthorized("ya no eres miembro de este workspace").into_response())
        }
        Err(e) => {
            tracing::error!(error = %e, "fallo al resolver el rol");
            Err(problem::internal("fallo al resolver el rol").into_response())
        }
    }
}

/// Resuelve `X-Workspace` (slug o id) contra las membresías del usuario.
#[allow(clippy::result_large_err)]
async fn resolver_por_header(
    state: &AppState,
    user_id: Uuid,
    crudo: &str,
) -> Result<(Option<Uuid>, Role), Response> {
    let por_slug = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT w.id, m.role FROM workspaces w \
         JOIN workspace_members m ON m.workspace_id = w.id \
         WHERE m.user_id = $1 AND w.slug = $2",
    )
    .bind(user_id)
    .bind(crudo)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "fallo al resolver X-Workspace");
        problem::internal("fallo al resolver el workspace").into_response()
    })?;
    let fila = if let Some(par) = por_slug {
        Some(par)
    } else if let Ok(id) = Uuid::parse_str(crudo) {
        sqlx::query_as::<_, (Uuid, String)>(
            "SELECT w.id, m.role FROM workspaces w \
             JOIN workspace_members m ON m.workspace_id = w.id \
             WHERE m.user_id = $1 AND w.id = $2",
        )
        .bind(user_id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "fallo al resolver X-Workspace");
            problem::internal("fallo al resolver el workspace").into_response()
        })?
    } else {
        None
    };
    match fila {
        Some((id, rol_texto)) => {
            let role =
                Role::from_db(&rol_texto).map_err(|e| problem::internal(e).into_response())?;
            Ok((Some(id), role))
        }
        None => Err(problem::unauthorized("no eres miembro de ese workspace").into_response()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn authed(workspace_id: Option<Uuid>) -> AuthedKey {
        AuthedKey {
            id: Uuid::now_v7(),
            name: "dev".to_string(),
            workspace_id,
            role: Role::Owner,
            principal: Principal::ApiKey,
        }
    }

    #[test]
    fn hash_key_produce_sha256_hex_conocido() {
        // Vector verificado contra `hash-key "dev-key-123"` en el smoke test
        assert_eq!(
            hash_key("dev-key-123"),
            "0f11c9ecaecabe613512ae472855ae9cb7d9639bc8c3fe85e0357efbf4739cd4"
        );
    }

    #[test]
    fn hash_key_es_determinista_y_distinto_por_entrada() {
        assert_eq!(hash_key("abc"), hash_key("abc"));
        assert_ne!(hash_key("abc"), hash_key("abd"));
        assert_eq!(hash_key("").len(), 64); // SHA-256 = 64 hex
    }

    #[test]
    fn una_key_sin_workspace_no_puede_usar_los_recursos() {
        let key = authed(None);
        let problem = key.workspace().expect_err("sin workspace");
        assert_eq!(problem.status, StatusCode::FORBIDDEN);
        assert!(
            problem.detail.unwrap().contains("workspace"),
            "el detalle debe nombrar el workspace"
        );
    }

    #[test]
    fn una_key_vinculada_devuelve_su_workspace() {
        let workspace_id = Uuid::now_v7();
        assert_eq!(
            authed(Some(workspace_id)).workspace().unwrap(),
            workspace_id
        );
    }

    #[test]
    fn los_roles_se_resuelven_desde_la_bd_y_rechazan_lo_desconocido() {
        assert_eq!(Role::from_db("owner"), Ok(Role::Owner));
        assert_eq!(Role::from_db("viewer"), Ok(Role::Viewer));
        assert!(Role::from_db("emperador").is_err());
    }

    #[test]
    fn la_matriz_de_roles_cubre_todas_las_rutas_del_plano() {
        use Role::{Editor, Owner, Runner, Viewer};
        // (método, ruta, rol mínimo)
        let casos = [
            // lectura: cualquier identidad
            ("GET", "/api/v1/bridges", Viewer),
            ("GET", "/api/v1/executions", Viewer),
            ("GET", "/api/v1/executions/x/trace", Viewer),
            ("GET", "/api/v1/credentials", Viewer),
            ("GET", "/api/v1/blocks", Viewer),
            ("GET", "/api/v1/whoami", Viewer),
            // ejecución: runner
            (
                "POST",
                "/api/v1/bridges/123e4567-e89b-12d3-a456-426614174000/run",
                Runner,
            ),
            (
                "POST",
                "/api/v1/bridges/123e4567-e89b-12d3-a456-426614174000/dry-run",
                Runner,
            ),
            (
                "POST",
                "/api/v1/executions/123e4567-e89b-12d3-a456-426614174000/replay",
                Runner,
            ),
            ("POST", "/api/v1/events/pedido.creado", Runner),
            // administración de recursos: editor
            ("POST", "/api/v1/blocks", Editor),
            ("POST", "/api/v1/blocks/x/versions", Editor),
            ("PATCH", "/api/v1/bridges/x", Editor),
            ("POST", "/api/v1/bridges/x/versions", Editor),
            ("POST", "/api/v1/credentials", Editor),
            ("PATCH", "/api/v1/credentials/x", Editor),
            ("DELETE", "/api/v1/credentials/x", Editor),
            ("POST", "/api/v1/bridges", Editor),
            ("DELETE", "/api/v1/bridges/x", Editor),
            // membresía: owner
            ("GET", "/api/v1/members", Owner),
            ("POST", "/api/v1/members", Owner),
            ("PATCH", "/api/v1/members/x", Owner),
            ("DELETE", "/api/v1/members/x", Owner),
        ];
        for (metodo, ruta, esperado) in casos {
            assert_eq!(rol_requerido(metodo, ruta), esperado, "{metodo} {ruta}");
        }
        // La jerarquía lineal: owner cubre todo; viewer solo lectura.
        assert!(
            Role::Owner.cubre(Editor) && Role::Editor.cubre(Runner) && Role::Runner.cubre(Viewer)
        );
        assert!(
            !Role::Viewer.cubre(Runner)
                && !Role::Runner.cubre(Editor)
                && !Role::Editor.cubre(Owner)
        );
    }

    #[test]
    fn el_email_solo_existe_en_el_principal_de_usuario() {
        let usuario = AuthedKey {
            id: Uuid::now_v7(),
            name: "ada@ejemplo.dev".to_string(),
            workspace_id: Some(Uuid::now_v7()),
            role: Role::Editor,
            principal: Principal::User {
                user_id: Uuid::now_v7(),
                email: "ada@ejemplo.dev".to_string(),
            },
        };
        assert_eq!(usuario.email(), Some("ada@ejemplo.dev"));
        assert_eq!(authed(Some(Uuid::now_v7())).email(), None);
    }
}
