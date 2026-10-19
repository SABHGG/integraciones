//! E2E de MVP4 (task 5.3) contra un Postgres **real** (BD `integraciones_e2e`).
//!
//! El hilo completo de identidad:
//!
//! 1. **Registro y alta**: jefa se registra; el workspace la tiene como
//!    `owner`; añade a edu (cuenta existente) como `editor` y a viga como
//!    `viewer` por el plano.
//! 2. **Login por rol**: cada quien entra con su workspace fijado.
//! 3. **Editor publica**: edu valida el borrador y publica la versión 1.
//! 4. **Viewer denegado**: viga lee, pero su disparo es 403 con el rol.
//! 5. **Auditoría**: el replay de la ejecución de edu queda a su nombre
//!    (`trigger_principal` user/email).
//! 6. **Expulsión revoca**: jefa expulsa a edu; su token muere al instante.
//!
//! Si `DATABASE_URL` no está definida los tests se omiten.

use api::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

static DB_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

struct Mundo {
    app: Router,
    pool: PgPool,
    state: AppState,
    workspace_id: Uuid,
    workspace_slug: String,
    /// Turno de base de datos, vivo mientras dura el test.
    _turno: MutexGuard<'static, ()>,
}

async fn begin() -> Option<Mundo> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("DATABASE_URL no está definida: se omite el e2e de MVP4");
            return None;
        }
    };
    let url = url
        .rsplit_once('/')
        .map(|(base, _db)| format!("{base}/integraciones_e2e"))
        .expect("DATABASE_URL con nombre de base de datos");

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("conexión a Postgres (¿corriste ./scripts/dev-postgres.sh?)");
    sqlx::migrate!("../api/migrations")
        .run(&pool)
        .await
        .expect("migraciones aplicadas");

    let turno = DB_LOCK.lock().await;
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'e2e4-%'")
        .execute(&pool)
        .await
        .expect("limpieza");
    sqlx::query("DELETE FROM users WHERE email LIKE '%e2e4.invalid'")
        .execute(&pool)
        .await
        .expect("limpieza de usuarios");

    let workspace_id = Uuid::now_v7();
    let workspace_slug = format!("e2e4-{workspace_id}");
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace e2e MVP4")
        .bind(&workspace_slug)
        .execute(&pool)
        .await
        .expect("workspace");

    let app = api::app(AppState { pool: pool.clone() });
    Some(Mundo {
        app,
        state: AppState { pool: pool.clone() },
        pool,
        workspace_id,
        workspace_slug,
        _turno: turno,
    })
}

impl Mundo {
    async fn send(
        &self,
        method: &str,
        uri: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
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
            .oneshot(builder.body(bytes).expect("petición"))
            .await
            .expect("respuesta");
        let status = response.status();
        let text = String::from_utf8_lossy(
            &axum::body::to_bytes(response.into_body(), 1 << 20)
                .await
                .expect("cuerpo"),
        )
        .into_owned();
        let json = serde_json::from_str(&text).unwrap_or(Value::Null);
        (status, json)
    }

    /// Registro público: devuelve (email, password, token sin workspace).
    async fn registrar(&self, nombre: &str) -> (String, String, String) {
        let email = format!("{nombre}-{}@e2e4.invalid", Uuid::now_v7().simple());
        let password = "contraseña-larga-suficiente";
        let (status, cuerpo) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                None,
                Some(json!({ "email": email, "password": password })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{cuerpo}");
        let token = cuerpo["token"].as_str().expect("token").to_string();
        (email, password.to_string(), token)
    }

    /// Login con el workspace de test fijado; devuelve el token.
    async fn login(&self, email: &str, password: &str) -> String {
        let (status, cuerpo) = self
            .send(
                "POST",
                "/api/v1/auth/login",
                None,
                Some(json!({
                    "email": email, "password": password, "workspace": self.workspace_slug
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{cuerpo}");
        cuerpo["token"].as_str().expect("token").to_string()
    }

    /// El owner de turno (una sesión de usuario con rol owner en el workspace).
    async fn alta_owner(&self, nombre: &str) -> (String, String, String) {
        let (email, password, token_sin_ws) = self.registrar(nombre).await;
        // Sin key de administración: el PRIMER owner se siembra por SQL (no
        // hay nadie con permisos aún); los siguientes entran por el plano.
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) \
             SELECT $1, id, 'owner' FROM users WHERE email = $2",
        )
        .bind(self.workspace_id)
        .bind(&email)
        .execute(&self.pool)
        .await
        .expect("membresía owner");
        let _ = token_sin_ws;
        let token = self.login(&email, &password).await;
        (email, password, token)
    }

    /// Puente con borrador VÁLIDO (bloque de biblioteca real) para publicar.
    async fn sembrar_borrador_publicable(&self, slug: &str) -> Uuid {
        let bridge_id = Uuid::now_v7();
        let block_id = Uuid::now_v7();
        let block_version_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO blocks (id, workspace_id, slug, name, block_type)
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
            "INSERT INTO block_versions (id, block_id, version, code)
             VALUES ($1, $2, 1, 'export default () => ({ ok: true });')",
        )
        .bind(block_version_id)
        .bind(block_id)
        .execute(&self.pool)
        .await
        .expect("block_version");
        let draft = json!({
            "trigger": { "type": "http", "method": "POST", "path": format!("/x/{slug}") },
            "blocks": [{
                "id": "b1", "name": "eco",
                "blockId": block_id, "blockVersionId": block_version_id,
                "input": {}, "config": {}
            }]
        });
        sqlx::query(
            "INSERT INTO bridges (id, workspace_id, slug, name, draft) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(bridge_id)
        .bind(self.workspace_id)
        .bind(slug)
        .bind(format!("Puente {slug}"))
        .bind(&draft)
        .execute(&self.pool)
        .await
        .expect("bridge");
        bridge_id
    }
}

#[tokio::test]
async fn e2e_mvp4_identidad_roles_miembros_y_auditoria() {
    let Some(mundo) = begin().await else { return };

    // ── 1. Registro + alta de owner (sembrada: primera identidad) ────────
    let (_email_jefa, _password_jefa, token_jefa) = mundo.alta_owner("jefa").await;
    let whoami = mundo
        .send("GET", "/api/v1/whoami", Some(&token_jefa), None)
        .await;
    assert_eq!(whoami.1["principal"]["type"], json!("user"));
    assert_eq!(whoami.1["principal"]["role"], json!("owner"));

    // ── 2. La jefa añade edu (editor) y viga (viewer) por el plano ───────
    let (email_edu, password_edu, _) = mundo.registrar("edu").await;
    let (status, cuerpo) = mundo
        .send(
            "POST",
            "/api/v1/members",
            Some(&token_jefa),
            Some(json!({ "email": email_edu, "role": "editor" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{cuerpo}");

    let (email_viga, password_viga, _) = mundo.registrar("viga").await;
    let (status, _) = mundo
        .send(
            "POST",
            "/api/v1/members",
            Some(&token_jefa),
            Some(json!({ "email": email_viga, "role": "viewer" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let token_edu = mundo.login(&email_edu, &password_edu).await;
    let token_viga = mundo.login(&email_viga, &password_viga).await;

    // ── 3. El editor publica la versión 1 del borrador ───────────────────
    let puente = mundo.sembrar_borrador_publicable("publicable").await;
    let (status, publicado) = mundo
        .send(
            "POST",
            &format!("/api/v1/bridges/{puente}/versions"),
            Some(&token_edu),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{publicado}");
    assert_eq!(publicado["version"], json!(1));

    // ── 4. El viewer lee pero su disparo es 403 nombrando runner ─────────
    let (status, _) = mundo
        .send("GET", "/api/v1/bridges", Some(&token_viga), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, cuerpo) = mundo
        .send(
            "POST",
            &format!("/api/v1/bridges/{puente}/run"),
            Some(&token_viga),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{cuerpo}");
    assert!(cuerpo.to_string().contains("runner"), "{cuerpo}");

    // ── 5. El editor dispara y re-ejecuta: el replay queda a su nombre ───
    let (status, ack) = mundo
        .send(
            "POST",
            &format!("/api/v1/bridges/{puente}/run"),
            Some(&token_edu),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{ack}");
    let execution_edu = ack["executionId"]
        .as_str()
        .expect("executionId")
        .to_string();

    let (status, _) = mundo
        .send(
            "POST",
            &format!("/api/v1/executions/{execution_edu}/replay"),
            Some(&token_edu),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (principal,): (Value,) = sqlx::query_as(
        "SELECT trigger_principal FROM executions \
         WHERE bridge_id = $1 AND trigger_type = 'manual' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(puente)
    .fetch_one(&mundo.pool)
    .await
    .expect("replay de edu");
    assert_eq!(principal["type"], json!("user"));
    assert_eq!(principal["email"], json!(email_edu));

    // ── 6. La expulsión revoca el acceso al instante ─────────────────────
    let edu_id = mundo
        .send("GET", "/api/v1/members", Some(&token_jefa), None)
        .await
        .1["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["email"] == json!(email_edu))
        .expect("edu")["userId"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = mundo
        .send(
            "DELETE",
            &format!("/api/v1/members/{edu_id}"),
            Some(&token_jefa),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = mundo
        .send("GET", "/api/v1/bridges", Some(&token_edu), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "el token de edu murió");

    // Higiene: drena la cola global del e2e.
    while api::worker::procesar_pendiente(&mundo.state)
        .await
        .expect("worker")
    {}
}
