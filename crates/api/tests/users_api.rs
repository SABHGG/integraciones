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

// ── matriz de roles (MVP4, fase 2, spec `workspace-roles`) ─────────────

impl Harness {
    /// Crea un usuario con el rol dado en el workspace de test y devuelve
    /// su token de sesión (con el workspace ya fijado).
    async fn token_con_rol(&self, nombre: &str, rol: &str) -> String {
        let (email, password, _) = self.registrar(nombre).await;
        self.sembrar_miembro(&email, rol).await;
        let (status, cuerpo) = self
            .post(
                "/api/v1/auth/login",
                None,
                Some(json!({ "email": email, "password": password, "workspace": self.workspace_slug })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{cuerpo}");
        let sobre: Value = serde_json::from_str(&cuerpo).expect("JSON de login");
        sobre["token"].as_str().expect("token").to_string()
    }

    /// Petición que devuelve (status, cuerpo) — cómoda para los tests de roles.
    async fn http(
        &self,
        method: &str,
        uri: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, String) {
        let respuesta = self.enviar(method, uri, bearer, body).await;
        (respuesta.status, respuesta.body)
    }

    /// Siembra un puente publicado v1 (eco JS) para disparos, con el mismo
    /// contenido como borrador válido (el dry-run ensaya el borrador y exige
    /// blockId/blockVersionId reales).
    async fn sembrar_puente_publicado(&self, slug: &str) -> Uuid {
        let bridge_id = Uuid::now_v7();
        let block_id = Uuid::now_v7();
        let block_version_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO blocks (id, workspace_id, slug, name, block_type) \
             VALUES ($1, $2, $3, $4, 'custom_logic')",
        )
        .bind(block_id)
        .bind(self.workspace_id)
        .bind(format!("eco-{slug}"))
        .bind(format!("Eco {slug}"))
        .execute(&self.pool)
        .await
        .expect("bloque");
        sqlx::query(
            "INSERT INTO block_versions (id, block_id, version, code) \
             VALUES ($1, $2, 1, 'export default () => ({ ok: true });')",
        )
        .bind(block_version_id)
        .bind(block_id)
        .execute(&self.pool)
        .await
        .expect("block_version");
        let definition = json!({
            "trigger": { "type": "http", "method": "POST", "path": format!("/x/{slug}") },
            "blocks": [{
                "id": "b1", "name": "eco",
                "blockId": block_id,
                "blockVersionId": block_version_id,
                "input": {}, "config": {}, "outputSchema": {}
            }]
        });
        sqlx::query(
            "INSERT INTO bridges (id, workspace_id, slug, name, draft) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(bridge_id)
        .bind(self.workspace_id)
        .bind(slug)
        .bind(format!("Puente {slug}"))
        .bind(&definition)
        .execute(&self.pool)
        .await
        .expect("bridge");
        sqlx::query(
            "INSERT INTO bridge_versions (id, bridge_id, version, definition) VALUES ($1, $2, 1, $3)",
        )
        .bind(Uuid::now_v7())
        .bind(bridge_id)
        .bind(&definition)
        .execute(&self.pool)
        .await
        .expect("bridge_version");
        sqlx::query("UPDATE bridges SET latest_version = 1 WHERE id = $1")
            .bind(bridge_id)
            .execute(&self.pool)
            .await
            .expect("latest_version");
        bridge_id
    }
}

#[tokio::test]
async fn el_viewer_solo_lee_y_el_resto_es_403_con_el_rol_requerido() {
    let Some(h) = begin().await else { return };
    let token = h.token_con_rol("viewer", "viewer").await;

    // Lectura: OK.
    let lectura = h.get("/api/v1/bridges", Some(&token), None).await;
    assert_eq!(lectura.status, StatusCode::OK, "{}", lectura.body);

    // Escrituras de recursos: 403 con el rol requerido en el detalle.
    for (metodo, uri, cuerpo) in [
        (
            "POST",
            "/api/v1/blocks",
            Some(json!({"slug": "x", "name": "x", "blockType": "custom_logic"})),
        ),
        (
            "POST",
            "/api/v1/credentials",
            Some(json!({"name": "c", "secret": "secreto-largo"})),
        ),
        (
            "DELETE",
            "/api/v1/credentials/123e4567-e89b-12d3-a456-426614174000",
            None,
        ),
    ] {
        let (status, cuerpo_respuesta) = h.http(metodo, uri, Some(&token), cuerpo).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{metodo} {uri}: {cuerpo_respuesta}"
        );
        assert!(
            cuerpo_respuesta.contains("editor"),
            "el 403 debe nombrar el rol requerido: {cuerpo_respuesta}"
        );
    }

    // Ejecución también supera su techo (viewer < runner).
    let puente = h.sembrar_puente_publicado("para-viewer").await;
    let (status, cuerpo_respuesta) = h
        .http(
            "POST",
            &format!("/api/v1/bridges/{puente}/run"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{cuerpo_respuesta}");
    assert!(cuerpo_respuesta.contains("runner"), "{cuerpo_respuesta}");
}

#[tokio::test]
async fn el_runner_ejecuta_pero_no_administra_recursos() {
    let Some(h) = begin().await else { return };
    let token = h.token_con_rol("runner", "runner").await;
    let puente = h.sembrar_puente_publicado("para-runner").await;

    // Ejecutar: disparo manual, dry-run y publicación de evento.
    let (status, _) = h
        .http(
            "POST",
            &format!("/api/v1/bridges/{puente}/run"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "el disparo manual es de runner"
    );

    let (status, cuerpo) = h
        .http(
            "POST",
            &format!("/api/v1/bridges/{puente}/dry-run"),
            Some(&token),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cuerpo}");

    let (status, _) = h
        .http(
            "POST",
            "/api/v1/events/runner.topic",
            Some(&token),
            Some(json!({"x": 1})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "publicar eventos es de runner"
    );

    // Administrar recursos: 403.
    let (status, cuerpo) = h
        .http(
            "POST",
            "/api/v1/blocks",
            Some(&token),
            Some(json!({"slug": "no-puede", "name": "no", "blockType": "custom_logic"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{cuerpo}");
}

#[tokio::test]
async fn el_editor_administra_recursos_pero_no_la_membresia() {
    let Some(h) = begin().await else { return };
    let token = h.token_con_rol("editor", "editor").await;

    // Crear bloque (recurso): OK.
    let (status, cuerpo) = h
        .http(
            "POST",
            "/api/v1/blocks",
            Some(&token),
            Some(json!({"slug": "de-editor", "name": "de editor", "blockType": "custom_logic"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{cuerpo}");

    // Membresía: 403 con rol owner (la matriz corre antes del 404 de ruta).
    let (status, cuerpo) = h
        .http(
            "PATCH",
            "/api/v1/members/123e4567-e89b-12d3-a456-426614174000",
            Some(&token),
            Some(json!({"role": "viewer"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{cuerpo}");
    assert!(cuerpo.contains("owner"), "{cuerpo}");
}

// ── membresía y auditoría (MVP4, fase 3, spec `workspace-roles`) ───────

#[tokio::test]
async fn el_owner_gestiona_la_membresia_y_el_resto_la_ve_en_lectura() {
    let Some(h) = begin().await else { return };
    let (email_nuevo, password_nuevo, _) = h.registrar("invitado").await;

    // El owner (api key) añade por email existente.
    let (status, cuerpo) = h
        .post(
            "/api/v1/members",
            Some(&h.key),
            Some(json!({ "email": email_nuevo, "role": "editor" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{cuerpo}");

    // Añadir un email SIN cuenta: 404 claro.
    let fantasma = format!("fantasma-{}{EMAIL_SUFFIX}", Uuid::now_v7().simple());
    let (status, cuerpo) = h
        .post(
            "/api/v1/members",
            Some(&h.key),
            Some(json!({ "email": fantasma, "role": "viewer" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{cuerpo}");
    assert!(cuerpo.contains("registrarse"), "{cuerpo}");

    // Duplicado: 409.
    let (status, _) = h
        .post(
            "/api/v1/members",
            Some(&h.key),
            Some(json!({ "email": email_nuevo, "role": "viewer" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // El editor (usuario) VE la lista (GET = viewer) con email y rol.
    let token_editor = h.token_con_rol("editor-vista", "editor").await;
    let lista = h.get("/api/v1/members", Some(&token_editor), None).await;
    assert_eq!(lista.status, StatusCode::OK, "{}", lista.body);
    let lista_json = lista.json();
    let miembros = lista_json["members"].as_array().expect("members");
    assert!(
        miembros
            .iter()
            .any(|m| m["email"] == json!(email_nuevo) && m["role"] == json!("editor")),
        "{}",
        lista.body
    );

    // El nuevo miembro entra con su workspace ya fijado por login.
    let (status, cuerpo) = h
        .post(
            "/api/v1/auth/login",
            None,
            Some(json!({ "email": email_nuevo, "password": password_nuevo, "workspace": h.workspace_slug })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cuerpo}");

    // Owner cambia el rol y expulsa.
    let user_id = h.get("/api/v1/members", Some(&h.key), None).await.json()["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["email"] == json!(email_nuevo))
        .expect("miembro nuevo")["userId"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = h
        .http(
            "PATCH",
            &format!("/api/v1/members/{user_id}"),
            Some(&h.key),
            Some(json!({ "role": "viewer" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "cambio de rol");
    let (status, _) = h
        .http(
            "DELETE",
            &format!("/api/v1/members/{user_id}"),
            Some(&h.key),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "expulsión");
}

#[tokio::test]
async fn el_ultimo_owner_no_se_degrada_ni_se_expulsa() {
    let Some(h) = begin().await else { return };
    let (email_jefe, _, _) = h.registrar("jefe").await;
    let (status, _) = h
        .post(
            "/api/v1/members",
            Some(&h.key),
            Some(json!({ "email": email_jefe, "role": "owner" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let jefe_id = h.get("/api/v1/members", Some(&h.key), None).await.json()["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["email"] == json!(email_jefe))
        .expect("jefe")["userId"]
        .as_str()
        .unwrap()
        .to_string();

    // Último owner: degradar → 409; expulsar → 409.
    let (status, cuerpo) = h
        .http(
            "PATCH",
            &format!("/api/v1/members/{jefe_id}"),
            Some(&h.key),
            Some(json!({ "role": "viewer" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{cuerpo}");
    let (status, _) = h
        .http(
            "DELETE",
            &format!("/api/v1/members/{jefe_id}"),
            Some(&h.key),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Con un segundo owner, degradar al primero ya es posible.
    let (email_socio, _, _) = h.registrar("socio").await;
    let (status, _) = h
        .post(
            "/api/v1/members",
            Some(&h.key),
            Some(json!({ "email": email_socio, "role": "owner" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = h
        .http(
            "PATCH",
            &format!("/api/v1/members/{jefe_id}"),
            Some(&h.key),
            Some(json!({ "role": "viewer" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "ya no es el último owner");
}

#[tokio::test]
async fn la_ejecucion_audita_quien_disparo() {
    let Some(h) = begin().await else { return };
    let puente = h.sembrar_puente_publicado("auditada-mvp4").await;

    // 1. Usuario (editor) dispara: principal user con su email.
    let token = h.token_con_rol("disparador", "editor").await;
    let (status, cuerpo) = h
        .http(
            "POST",
            &format!("/api/v1/bridges/{puente}/run"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{cuerpo}");
    // Buscar la ejecución del puente y ver su principal.
    let (principal,): (Value,) = sqlx::query_as(
        "SELECT trigger_principal FROM executions e \
         JOIN bridges b ON b.id = e.bridge_id \
         WHERE b.slug = 'auditada-mvp4' ORDER BY e.created_at DESC LIMIT 1",
    )
    .fetch_one(&h.pool)
    .await
    .expect("ejecución del editor");
    assert_eq!(principal["type"], json!("user"));
    assert!(
        principal["email"]
            .as_str()
            .unwrap_or("")
            .ends_with(EMAIL_SUFFIX),
        "{}",
        principal
    );
    // 2. API key dispara (replay de la misma ejecución): principal api_key.
    let execution_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT e.id FROM executions e JOIN bridges b ON b.id = e.bridge_id \
         WHERE b.slug = 'auditada-mvp4' ORDER BY e.created_at DESC LIMIT 1",
    )
    .fetch_one(&h.pool)
    .await
    .expect("id de ejecución");
    let (status, _) = h
        .post(
            &format!("/api/v1/executions/{execution_id}/replay"),
            Some(&h.key),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (principal_replay,): (Value,) = sqlx::query_as(
        "SELECT trigger_principal FROM executions \
         WHERE bridge_id = $1 AND trigger_type = 'manual' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(puente)
    .fetch_one(&h.pool)
    .await
    .expect("replay");
    assert_eq!(principal_replay["type"], json!("api_key"));
    assert_eq!(principal_replay["name"], json!("users-test-key"));

    // 3. Fan-out por emit interno: system.
}
