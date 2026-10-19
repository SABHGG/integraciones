//! Tests de usuarios y sesiones (MVP4, fase 1, spec `user-auth`).
//!
//! Registro, login con/workspace, logout con revocación, whoami por tipo de
//! principal y sesión multi-workspace con `X-Workspace`.
//!
//! Si `DATABASE_URL` no está definida (CI sin base de datos) los tests se
//! omiten. En local:
//!
//! ```text
//! ./scripts/dev-postgres.sh
//! DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones cargo test -p api --test users_api
//! ```

use api::auth::hash_key;
use api::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

/// Los tests comparten base de datos: se serializan.
static DB_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// Prefijo de emails exclusivo de estos tests (limpieza determinista).
const EMAIL_SUFFIX: &str = "@users-test.invalid";

/// Respuesta ya leída: status y cuerpo crudo.
struct Resp {
    status: StatusCode,
    body: String,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("el cuerpo no es JSON ({e}): {}", self.body))
    }
}

struct Harness {
    app: Router,
    pool: PgPool,
    workspace_id: Uuid,
    workspace_slug: String,
    /// Key de administrador sembrada (flujo clásico por API key).
    key: String,
    /// Turno de base de datos, vivo mientras dura el test.
    _turno: MutexGuard<'static, ()>,
}

async fn begin() -> Option<Harness> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!(
                "DATABASE_URL no está definida: se omiten los tests de integración de la API"
            );
            return None;
        }
    };

    let pool = api::connect(&url)
        .await
        .expect("conexión a Postgres (¿corriste ./scripts/dev-postgres.sh?)");
    api::migrate(&pool).await.expect("migraciones aplicadas");

    let turno = DB_LOCK.lock().await;

    // Limpieza por prefijos dedicados (cascada desde workspaces/users).
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'users-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza de workspaces");
    sqlx::query("DELETE FROM users WHERE email LIKE '%users-test.invalid'")
        .execute(&pool)
        .await
        .expect("limpieza de usuarios");
    sqlx::query("DELETE FROM api_keys WHERE name LIKE 'users-test%'")
        .execute(&pool)
        .await
        .expect("limpieza de keys");

    let workspace_id = Uuid::now_v7();
    let workspace_slug = format!("users-test-{workspace_id}");
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace de test")
        .bind(&workspace_slug)
        .execute(&pool)
        .await
        .expect("workspace");

    let key = format!("users-test-key-{workspace_id}");
    sqlx::query("INSERT INTO api_keys (id, workspace_id, name, key_hash) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::now_v7())
        .bind(workspace_id)
        .bind("users-test-key")
        .bind(hash_key(&key))
        .execute(&pool)
        .await
        .expect("api key");

    Some(Harness {
        app: api::app(AppState { pool: pool.clone() }),
        pool,
        workspace_id,
        workspace_slug,
        key,
        _turno: turno,
    })
}

impl Harness {
    async fn enviar(
        &self,
        method: &str,
        uri: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> Resp {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(bearer) = bearer {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
        }
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let bytes = body
            .map(|v| Body::from(v.to_string()))
            .unwrap_or(Body::empty());
        let response = self
            .app
            .clone()
            .oneshot(builder.body(bytes).expect("petición válida"))
            .await
            .expect("respuesta del router");
        let status = response.status();
        let cuerpo = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("cuerpo legible");
        Resp {
            status,
            body: String::from_utf8_lossy(&cuerpo).into_owned(),
        }
    }

    async fn post(
        &self,
        uri: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, String) {
        let respuesta = self.enviar("POST", uri, bearer, body).await;
        (respuesta.status, respuesta.body)
    }

    async fn get(&self, uri: &str, bearer: Option<&str>, workspace: Option<&str>) -> Resp {
        let mut builder = Request::builder().method("GET").uri(uri);
        if let Some(bearer) = bearer {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
        }
        if let Some(workspace) = workspace {
            builder = builder.header("x-workspace", workspace);
        }
        let response = self
            .app
            .clone()
            .oneshot(builder.body(Body::empty()).expect("petición válida"))
            .await
            .expect("respuesta del router");
        let status = response.status();
        let cuerpo = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("cuerpo legible");
        Resp {
            status,
            body: String::from_utf8_lossy(&cuerpo).into_owned(),
        }
    }

    /// Registra un usuario nuevo y devuelve (email, password, token).
    async fn registrar(&self, nombre: &str) -> (String, String, String) {
        let email = format!(
            "{nombre}-{uuid}{EMAIL_SUFFIX}",
            uuid = Uuid::now_v7().simple()
        );
        let password = "contraseña-larga-suficiente";
        let (status, cuerpo) = self
            .post(
                "/api/v1/auth/register",
                None,
                Some(json!({ "email": email, "password": password })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{cuerpo}");
        let sobre: Value = serde_json::from_str(&cuerpo).expect("JSON de registro");
        let token = sobre["token"].as_str().expect("token").to_string();
        (email, password.to_string(), token)
    }

    /// Siembra una membresía del workspace de test con el rol dado.
    async fn sembrar_miembro(&self, email: &str, rol: &str) {
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) \
             SELECT $1, id, $3 FROM users WHERE email = $2",
        )
        .bind(self.workspace_id)
        .bind(email)
        .bind(rol)
        .execute(&self.pool)
        .await
        .expect("membresía sembrada");
    }
}

#[tokio::test]
async fn el_registro_crea_la_cuenta_con_sesion_sin_workspace() {
    let Some(h) = begin().await else { return };

    let (status, cuerpo) = h
        .post(
            "/api/v1/auth/register",
            None,
            Some(json!({
                "email": format!("nueva-{}{EMAIL_SUFFIX}", Uuid::now_v7().simple()),
                "password": "contraseña-larga-suficiente"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{cuerpo}");
    let sobre: Value = serde_json::from_str(&cuerpo).expect("JSON");
    assert!(sobre["user"]["id"].as_str().is_some());
    assert!(sobre["token"].as_str().is_some());
    assert!(sobre["expiresAt"].as_str().is_some());
    assert_eq!(sobre["workspaces"], json!([]), "crear cuenta no da acceso");

    // La sesión sin workspace exige elegirlo para usar recursos.
    let token = sobre["token"].as_str().unwrap();
    let whoami = h.get("/api/v1/whoami", Some(token), None).await;
    assert_eq!(whoami.status, StatusCode::UNAUTHORIZED, "{}", whoami.body);
    assert!(whoami.body.contains("X-Workspace"), "{}", whoami.body);
}

#[tokio::test]
async fn el_registro_valida_email_duplicado_contrasena_corta_y_formato() {
    let Some(h) = begin().await else { return };
    let (email, _, _) = h.registrar("duplicada").await;

    // Email duplicado → 409.
    let (status, cuerpo) = h
        .post(
            "/api/v1/auth/register",
            None,
            Some(json!({ "email": email, "password": "contraseña-larga-suficiente" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{cuerpo}");
    assert!(cuerpo.contains("ya existe"), "{cuerpo}");

    // Contraseña corta → 400.
    let (status, cuerpo) = h
        .post(
            "/api/v1/auth/register",
            None,
            Some(json!({ "email": format!("corta-{}{EMAIL_SUFFIX}", Uuid::now_v7().simple()), "password": "corta" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{cuerpo}");
    assert!(cuerpo.contains("10 caracteres"), "{cuerpo}");

    // Email inválido → 400.
    let (status, _) = h
        .post(
            "/api/v1/auth/register",
            None,
            Some(json!({ "email": "no-es-un-email", "password": "contraseña-larga" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn el_login_es_uniforme_y_con_una_membresia_fija_el_workspace() {
    let Some(h) = begin().await else { return };
    let (email, password, _) = h.registrar("miembro").await;
    h.sembrar_miembro(&email, "editor").await;

    // Contraseña incorrecta y email inexistente: MISMO 401.
    let (status_mala, cuerpo_mala) = h
        .post(
            "/api/v1/auth/login",
            None,
            Some(json!({ "email": email, "password": "no-es-la-contraseña" })),
        )
        .await;
    assert_eq!(status_mala, StatusCode::UNAUTHORIZED);
    let (status_inexistente, cuerpo_inexistente) = h
        .post(
            "/api/v1/auth/login",
            None,
            Some(json!({ "email": format!("fantasma-{}{EMAIL_SUFFIX}", Uuid::now_v7().simple()), "password": "da-igual-1234" })),
        )
        .await;
    assert_eq!(status_inexistente, StatusCode::UNAUTHORIZED);
    assert!(
        cuerpo_mala.contains("inválidas") && cuerpo_inexistente.contains("inválidas"),
        "el mensaje no distingue email inexistente de contraseña incorrecta"
    );

    // Login correcto: el workspace único es implícito.
    let (status, cuerpo) = h
        .post(
            "/api/v1/auth/login",
            None,
            Some(json!({ "email": email, "password": password })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cuerpo}");
    let sobre: Value = serde_json::from_str(&cuerpo).expect("JSON");
    let token = sobre["token"].as_str().expect("token").to_string();
    assert_eq!(sobre["activeWorkspace"]["slug"], json!(h.workspace_slug));
    assert_eq!(sobre["activeWorkspace"]["role"], json!("editor"));
    assert_eq!(sobre["workspaces"][0]["role"], json!("editor"));

    // El token resuelve identidad y rol en whoami, y usa el plano admin.
    let whoami = h.get("/api/v1/whoami", Some(&token), None).await;
    assert_eq!(whoami.status, StatusCode::OK, "{}", whoami.body);
    let principal = whoami.json()["principal"].clone();
    assert_eq!(principal["type"], json!("user"));
    assert_eq!(principal["email"], json!(email));
    assert_eq!(principal["role"], json!("editor"));

    let puentes = h.get("/api/v1/bridges", Some(&token), None).await;
    assert_eq!(puentes.status, StatusCode::OK, "{}", puentes.body);
}

#[tokio::test]
async fn con_varias_membresias_el_login_exige_elegir_workspace() {
    let Some(h) = begin().await else { return };
    let (email, password, _) = h.registrar("multi").await;
    h.sembrar_miembro(&email, "viewer").await;

    // Segundo workspace con otra membresía.
    let workspace2 = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace2)
        .bind("segundo workspace")
        .bind(format!("users-test-{workspace2}"))
        .execute(&h.pool)
        .await
        .expect("workspace 2");
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) \
         SELECT $1, id, 'owner' FROM users WHERE email = $2",
    )
    .bind(workspace2)
    .bind(&email)
    .execute(&h.pool)
    .await
    .expect("membresía 2");

    // Login sin elegir: token sin workspace + ambas membresías listadas.
    let (status, cuerpo) = h
        .post(
            "/api/v1/auth/login",
            None,
            Some(json!({ "email": email, "password": password })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cuerpo}");
    let ambiguo: Value = serde_json::from_str(&cuerpo).expect("JSON");
    assert!(ambiguo["activeWorkspace"].is_null());
    assert_eq!(ambiguo["workspaces"].as_array().map(Vec::len), Some(2));
    let token_ambiguo = ambiguo["token"].as_str().unwrap();

    // El token sin workspace exige X-Workspace y resuelve el rol POR PETICIÓN.
    let viewer = h
        .get(
            "/api/v1/whoami",
            Some(token_ambiguo),
            Some(&h.workspace_slug),
        )
        .await;
    assert_eq!(viewer.status, StatusCode::OK, "{}", viewer.body);
    assert_eq!(viewer.json()["principal"]["role"], json!("viewer"));
    let owner = h
        .get(
            "/api/v1/whoami",
            Some(token_ambiguo),
            Some(&format!("users-test-{workspace2}")),
        )
        .await;
    assert_eq!(owner.json()["principal"]["role"], json!("owner"));

    // Un workspace ajeno → 401.
    let ajeno = h
        .get(
            "/api/v1/whoami",
            Some(token_ambiguo),
            Some("users-test-ajeno"),
        )
        .await;
    assert_eq!(ajeno.status, StatusCode::UNAUTHORIZED);

    // Login ELEGIENDO workspace: la sesión nace fijada.
    let (status, cuerpo) = h
        .post(
            "/api/v1/auth/login",
            None,
            Some(json!({
                "email": email, "password": password, "workspace": h.workspace_slug
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cuerpo}");
    let fijado: Value = serde_json::from_str(&cuerpo).expect("JSON");
    assert_eq!(fijado["activeWorkspace"]["slug"], json!(h.workspace_slug));
    let token_fijado = fijado["token"].as_str().unwrap();
    let directo = h.get("/api/v1/whoami", Some(token_fijado), None).await;
    assert_eq!(directo.status, StatusCode::OK, "{}", directo.body);
    assert_eq!(directo.json()["principal"]["role"], json!("viewer"));

    // Elegir un workspace ajeno → 403.
    let (status, cuerpo) = h
        .post(
            "/api/v1/auth/login",
            None,
            Some(json!({ "email": email, "password": password, "workspace": "no-miembro" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{cuerpo}");
}

#[tokio::test]
async fn el_logout_revoca_la_sesion_y_el_token_muere() {
    let Some(h) = begin().await else { return };
    let (_, _, token) = h.registrar("salida").await;

    let (status, _) = h.post("/api/v1/auth/logout", Some(&token), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // El token ya no autentica nada.
    let despues = h.get("/api/v1/whoami", Some(&token), None).await;
    assert_eq!(despues.status, StatusCode::UNAUTHORIZED);

    // Logout con API key: no hay sesión que revocar, pero no es error (204).
    let (status, _) = h.post("/api/v1/auth/logout", Some(&h.key), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn la_api_key_sigue_autenticando_como_owner() {
    let Some(h) = begin().await else { return };

    let whoami = h.get("/api/v1/whoami", Some(&h.key), None).await;
    assert_eq!(whoami.status, StatusCode::OK, "{}", whoami.body);
    let principal = whoami.json()["principal"].clone();
    assert_eq!(principal["type"], json!("api_key"));
    assert_eq!(principal["role"], json!("owner"));

    let puentes = h.get("/api/v1/bridges", Some(&h.key), None).await;
    assert_eq!(puentes.status, StatusCode::OK, "{}", puentes.body);
}
