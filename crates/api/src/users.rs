//! Registro, login y logout de usuarios (MVP4, spec `user-auth`).
//!
//! Rutas PÚBLICAS montadas fuera del middleware `require_auth`: crear cuenta
//! no otorga acceso a ningún workspace. Las sesiones son opacas (token de 256
//! bits, guardado solo como SHA-256) y nacen ligadas a un workspace cuando el
//! usuario tiene exactamente uno; con varios, el login exige elegirlo.
#![allow(clippy::result_large_err)]

use crate::auth::{Principal, Role, hash_key};
use crate::problem::{self, Problem};
use crate::state::AppState;
use axum::Extension;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

/// Duración de la sesión (design D1): 30 días, sin extensión por uso en v4.
const DIAS_SESION: i32 = 30;

/// Contraseña mínima al registrar.
const MIN_PASSWORD: usize = 10;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registro {
    email: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Login {
    email: String,
    password: String,
    /// Workspace a fijar en la sesión (slug): obligatorio cuando el usuario
    /// pertenece a más de uno.
    #[serde(default)]
    workspace: Option<String>,
}

/// Argon2id (design D2): verificación en tiempo constante frente a cifrados.
fn hashear_password(password: &str) -> Result<String, Problem> {
    use argon2::Argon2;
    use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
    let sal = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &sal)
        .map(|hash| hash.to_string())
        .map_err(|error| {
            tracing::error!(%error, "fallo al hashear la contraseña");
            problem::internal("no se pudo registrar la contraseña")
        })
}

fn verificar_password(password: &str, hash: &str) -> bool {
    use argon2::Argon2;
    use argon2::password_hash::PasswordVerifier;
    match argon2::PasswordHash::new(hash) {
        Ok(parseado) => Argon2::default()
            .verify_password(password.as_bytes(), &parseado)
            .is_ok(),
        // Hash corrupto o de otro algoritmo: contraseña incorrecta (nunca 500).
        Err(_) => false,
    }
}

fn email_valido(email: &str) -> bool {
    // Validación mínima v4: algo@algo.dom (el único contacto es el owner que
    // añade por email; el delivering queda fuera del alcance).
    let partes: Vec<&str> = email.split('@').collect();
    partes.len() == 2
        && !partes[0].is_empty()
        && !partes[1].is_empty()
        && partes[1].contains('.')
        && email.len() <= 254
}

/// Membresías del usuario con su rol (para el login y la sesión sin fijar).
async fn membresias(
    state: &AppState,
    user_id: Uuid,
) -> Result<Vec<(Uuid, String, String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT w.id, w.slug, w.name, m.role FROM workspace_members m \
         JOIN workspaces w ON w.id = m.workspace_id \
         WHERE m.user_id = $1 ORDER BY w.slug",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
}

/// Crea la sesión: 256 bits de azar guardados solo como SHA-256.
async fn crear_sesion(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Option<Uuid>,
) -> Result<(String, chrono::DateTime<chrono::Utc>), Problem> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| {
        tracing::error!(%error, "fallo de azar para el token");
        problem::internal("no se pudo crear la sesión")
    })?;
    let token = hex::encode(bytes);
    let expires_at = chrono::Utc::now() + chrono::Duration::days(i64::from(DIAS_SESION));
    sqlx::query(
        "INSERT INTO user_sessions (id, user_id, workspace_id, token_hash, expires_at) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(workspace_id)
    .bind(hash_key(&token))
    .bind(expires_at)
    .execute(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "fallo al guardar la sesión");
        problem::internal("no se pudo crear la sesión")
    })?;
    Ok((token, expires_at))
}

fn respuesta_login(
    token: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
    activo: Option<&(Uuid, String, String, String)>,
    membresias: &[(Uuid, String, String, String)],
) -> Value {
    let workspaces: Vec<Value> = membresias
        .iter()
        .map(|(id, slug, name, rol)| {
            json!({
                "id": id,
                "slug": slug,
                "name": name,
                "role": rol,
            })
        })
        .collect();
    let activo_json = activo
        .map(|(id, slug, name, rol)| json!({ "id": id, "slug": slug, "name": name, "role": rol }));
    json!({
        "token": token,
        "expiresAt": expires_at.to_rfc3339(),
        "activeWorkspace": activo_json,
        "workspaces": workspaces,
    })
}

/// `POST /api/v1/auth/register`: crea el usuario y devuelve una sesión sin
/// workspace (crear cuenta no otorga acceso a ningún workspace).
async fn registrar(
    State(state): State<AppState>,
    Json(cuerpo): Json<Registro>,
) -> Result<(StatusCode, Json<Value>), Problem> {
    let email = cuerpo.email.trim().to_lowercase();
    if !email_valido(&email) {
        return Err(problem::bad_request("el email no es válido"));
    }
    if cuerpo.password.chars().count() < MIN_PASSWORD {
        return Err(problem::bad_request(format!(
            "la contraseña debe tener al menos {MIN_PASSWORD} caracteres"
        )));
    }
    let hash = hashear_password(&cuerpo.password)?;
    let user_id = Uuid::now_v7();
    let insertado = sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind(&email)
        .bind(&hash)
        .execute(&state.pool)
        .await;
    if let Err(sqlx::Error::Database(db)) = &insertado
        && db.constraint() == Some("users_email_key")
    {
        return Err(problem::conflict("ya existe una cuenta con ese email"));
    }
    insertado.map_err(|error| {
        tracing::error!(%error, "fallo al crear el usuario");
        problem::internal("no se pudo crear la cuenta")
    })?;

    let (token, expires_at) = crear_sesion(&state, user_id, None).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "user": { "id": user_id, "email": email },
            "token": token,
            "expiresAt": expires_at.to_rfc3339(),
            "workspaces": [],
        })),
    ))
}

/// `POST /api/v1/auth/login`: valida credenciales y emite la sesión. Con una
/// sola membresía el workspace es implícito; con varias exige `workspace`
/// (slug) salvo que solo una deba elegirse… siempre se devuelven las
/// membresías con su rol para que el cliente pueda elegir.
async fn entrar(
    State(state): State<AppState>,
    Json(cuerpo): Json<Login>,
) -> Result<Json<Value>, Problem> {
    let email = cuerpo.email.trim().to_lowercase();
    let fila: Option<(Uuid, Option<String>)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "fallo al buscar el usuario");
                problem::internal("fallo de autenticación")
            })?;

    // 401 uniforme: ni la existencia del email ni el motivo se revelan.
    let Some((user_id, hash)) = fila else {
        // Coste comparable al argon2 para no filtrar por tiempo.
        let _ = hashear_password(&cuerpo.password);
        return Err(problem::unauthorized("credenciales inválidas"));
    };
    let Some(hash_guardado) = hash else {
        // Cuenta sin contraseña definida (no debería existir en v4): 401 igual.
        return Err(problem::unauthorized("credenciales inválidas"));
    };
    if !verificar_password(&cuerpo.password, &hash_guardado) {
        return Err(problem::unauthorized("credenciales inválidas"));
    }

    let membresias = membresias(&state, user_id).await.map_err(|error| {
        tracing::error!(%error, "fallo al listar las membresías");
        problem::internal("fallo de autenticación")
    })?;

    // Fijar workspace: implícito con uno solo; elegido por slug con varios;
    // error 403 si el workspace pedido no es una membresía.
    let workspace_fijado = match (&cuerpo.workspace, membresias.len()) {
        (Some(slug), _) => {
            let elegido = membresias.iter().find(|(_, m_slug, _, _)| m_slug == slug);
            match elegido {
                Some(m) => Some(m.clone()),
                None => {
                    return Err(problem::forbidden("no eres miembro de ese workspace"));
                }
            }
        }
        (None, 1) => Some(membresias[0].clone()),
        (None, _) => None,
    };

    let (token, expires_at) = crear_sesion(
        &state,
        user_id,
        workspace_fijado.as_ref().map(|(id, _, _, _)| *id),
    )
    .await?;

    Ok(Json(respuesta_login(
        &token,
        expires_at,
        workspace_fijado.as_ref(),
        &membresias,
    )))
}

/// `POST /api/v1/auth/logout`: revoca la sesión del token presentado (204).
/// Solo aplica a sesiones de usuario; una API key no tiene sesión que revocar.
async fn salir(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, Problem> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let Some(token) = token else {
        return Err(problem::unauthorized(
            "Falta el header Authorization: Bearer",
        ));
    };
    sqlx::query(
        "UPDATE user_sessions SET revoked_at = now() \
         WHERE token_hash = $1 AND revoked_at IS NULL",
    )
    .bind(hash_key(token))
    .execute(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "fallo al revocar la sesión");
        problem::internal("no se pudo cerrar la sesión")
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// Router PÚBLICO de `/api/v1/auth` (montado fuera de `require_auth`):
/// registro y login no autentican; logout valida el token que trae.
pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/auth/register", axum::routing::post(registrar))
        .route("/auth/login", axum::routing::post(entrar))
        .route("/auth/logout", axum::routing::post(salir))
}

/// Principal expuesto para `whoami` (en `lib.rs`).
pub fn principal_json(principal: &Principal, role: &Role) -> Value {
    match principal {
        Principal::User { user_id, email } => json!({
            "type": "user",
            "userId": user_id,
            "email": email,
            "role": role.etiqueta(),
        }),
        Principal::ApiKey => json!({
            "type": "api_key",
            "role": role.etiqueta(),
        }),
    }
}

// ── gestión de miembros (MVP4, spec `workspace-roles`) ─────────────────
// Rutas AUTENTICADAS: se montan dentro del router admin (con require_auth);
// la matriz ya garantiza GET=viewer y mutaciones=owner.

/// Un miembro del workspace (email, rol, alta).
#[derive(serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct MiembroDto {
    user_id: Uuid,
    email: String,
    role: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// `GET /api/v1/members`: la membresía del workspace, visible por cualquier
/// identidad autenticada.
async fn listar_miembros(
    State(state): State<AppState>,
    Extension(key): Extension<crate::auth::AuthedKey>,
) -> Result<Json<Value>, Problem> {
    let workspace_id = key.workspace()?;
    let filas: Vec<MiembroDto> = sqlx::query_as(
        "SELECT u.id AS user_id, u.email, m.role, m.created_at \
         FROM workspace_members m JOIN users u ON u.id = m.user_id \
         WHERE m.workspace_id = $1 ORDER BY m.created_at, u.email",
    )
    .bind(workspace_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("listar los miembros: {error}")))?;
    Ok(Json(json!({ "members": filas })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AltaMiembro {
    email: String,
    role: String,
}

/// `POST /api/v1/members`: añade por email de usuario EXISTENTE (404 claro si
/// no hay cuenta). Solo owner (lo garantiza la matriz).
async fn anadir_miembro(
    State(state): State<AppState>,
    Extension(key): Extension<crate::auth::AuthedKey>,
    Json(cuerpo): Json<AltaMiembro>,
) -> Result<(StatusCode, Json<Value>), Problem> {
    let workspace_id = key.workspace()?;
    if Role::from_db(&cuerpo.role).is_err() {
        return Err(problem::bad_request(format!(
            "\"{}\" no es un rol (owner|editor|runner|viewer)",
            cuerpo.role
        )));
    }
    let email = cuerpo.email.trim().to_lowercase();
    let user_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await
        .map_err(|error| problem::internal(format!("buscar el usuario: {error}")))?;
    let Some(user_id) = user_id else {
        return Err(problem::not_found(format!(
            "no hay cuenta con {email}: esa persona debe registrarse primero"
        )));
    };
    let insertado = sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(&cuerpo.role)
    .execute(&state.pool)
    .await;
    if let Err(sqlx::Error::Database(db)) = &insertado
        && db.constraint().is_some()
    {
        return Err(problem::conflict("esa persona ya es miembro del workspace"));
    }
    insertado.map_err(|error| problem::internal(format!("añadir el miembro: {error}")))?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "userId": user_id, "email": email, "role": cuerpo.role })),
    ))
}

/// ¿Cuántos owners vivos tiene el workspace?
async fn owners_vivos(state: &AppState, workspace_id: Uuid) -> Result<i64, Problem> {
    sqlx::query_scalar(
        "SELECT count(*) FROM workspace_members \
         WHERE workspace_id = $1 AND role = 'owner'",
    )
    .bind(workspace_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("contar los owners: {error}")))
}

/// `PATCH /api/v1/members/{userId}`: cambia el rol. El último owner no se
/// degrada (409).
async fn cambiar_rol(
    State(state): State<AppState>,
    Extension(key): Extension<crate::auth::AuthedKey>,
    axum::extract::Path(user_id): axum::extract::Path<Uuid>,
    Json(cuerpo): Json<Value>,
) -> Result<Json<Value>, Problem> {
    let workspace_id = key.workspace()?;
    let Some(role) = cuerpo.get("role").and_then(Value::as_str) else {
        return Err(problem::bad_request("el body necesita \"role\""));
    };
    if Role::from_db(role).is_err() {
        return Err(problem::bad_request(format!(
            "\"{role}\" no es un rol (owner|editor|runner|viewer)"
        )));
    }
    let fila: Option<String> = sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("buscar el miembro: {error}")))?;
    let Some(rol_actual) = fila else {
        return Err(problem::not_found(
            "ese usuario no es miembro del workspace",
        ));
    };
    if rol_actual == "owner" && role != "owner" && owners_vivos(&state, workspace_id).await? <= 1 {
        return Err(problem::conflict(
            "es el último owner del workspace: añade otro owner antes de degradarlo",
        ));
    }
    let actualizado = sqlx::query(
        "UPDATE workspace_members SET role = $3 WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .execute(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("cambiar el rol: {error}")))?;
    if actualizado.rows_affected() == 0 {
        return Err(problem::not_found(
            "ese usuario no es miembro del workspace",
        ));
    }
    Ok(Json(json!({ "userId": user_id, "role": role })))
}

/// `DELETE /api/v1/members/{userId}`: expulsa. El último owner no se expulsa.
async fn expulsar(
    State(state): State<AppState>,
    Extension(key): Extension<crate::auth::AuthedKey>,
    axum::extract::Path(user_id): axum::extract::Path<Uuid>,
) -> Result<StatusCode, Problem> {
    let workspace_id = key.workspace()?;
    let rol_actual: Option<String> = sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("buscar el miembro: {error}")))?;
    let Some(rol_actual) = rol_actual else {
        return Err(problem::not_found(
            "ese usuario no es miembro del workspace",
        ));
    };
    if rol_actual == "owner" && owners_vivos(&state, workspace_id).await? <= 1 {
        return Err(problem::conflict(
            "es el último owner del workspace: añade otro owner antes de expulsarlo",
        ));
    }
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
        .bind(workspace_id)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .map_err(|error| problem::internal(format!("expulsar el miembro: {error}")))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Router AUTENTICADO de `/api/v1/members` (dentro del admin, tras la matriz).
pub fn router_miembros() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/members",
            axum::routing::get(listar_miembros).post(anadir_miembro),
        )
        .route(
            "/members/{user_id}",
            axum::routing::patch(cambiar_rol).delete(expulsar),
        )
}
