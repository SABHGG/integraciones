//! Tests del resumen de pasos en el listado (MVP6, task 3.1): el listado
//! `GET /executions` incluye por ejecución el estado y la duración de cada
//! paso (sin IO), en orden de cadena — el soporte del rastro en miniatura.
//! Ejecuciones sembradas por SQL (patrón de `stats_api`). Contra Postgres real.
//!
//! Si `DATABASE_URL` no está definida los tests se omiten.

use api::auth::hash_key;
use api::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

static DB_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());
const WS_PREFIX: &str = "pasos-test-";

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
    key: String,
    _turno: MutexGuard<'static, ()>,
}

async fn begin() -> Option<Harness> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("DATABASE_URL no está definida: se omiten los tests del resumen de pasos");
            return None;
        }
    };
    let pool = api::connect(&url)
        .await
        .expect("conexión a Postgres (¿corriste ./scripts/dev-postgres.sh?)");
    api::migrate(&pool).await.expect("migraciones aplicadas");

    let turno = DB_LOCK.lock().await;
    sqlx::query(&format!(
        "DELETE FROM workspaces WHERE slug LIKE '{WS_PREFIX}%'"
    ))
    .execute(&pool)
    .await
    .expect("limpieza");

    let workspace_id = Uuid::now_v7();
    let raw_key = format!("{WS_PREFIX}key-{workspace_id}");
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace pasos")
        .bind(format!("{WS_PREFIX}{workspace_id}"))
        .execute(&pool)
        .await
        .expect("workspace");
    sqlx::query("INSERT INTO api_keys (id, workspace_id, name, key_hash) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::now_v7())
        .bind(workspace_id)
        .bind("pasos-test-key")
        .bind(hash_key(&raw_key))
        .execute(&pool)
        .await
        .expect("api key");

    let app = api::app(AppState { pool: pool.clone() });
    Some(Harness {
        app,
        pool,
        workspace_id,
        key: raw_key,
        _turno: turno,
    })
}

impl Harness {
    async fn admin(&self, method: Method, uri: &str) -> Resp {
        let respuesta = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {}", self.key))
                    .body(Body::empty())
                    .expect("petición"),
            )
            .await
            .expect("respuesta");
        let status = respuesta.status();
        let body = String::from_utf8_lossy(
            &axum::body::to_bytes(respuesta.into_body(), 1 << 20)
                .await
                .expect("cuerpo"),
        )
        .into_owned();
        Resp { status, body }
    }

    async fn sembrar_puente(&self, slug: &str) -> (Uuid, Uuid) {
        let bridge_id = Uuid::now_v7();
        let version_id = Uuid::now_v7();
        sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
            .bind(bridge_id)
            .bind(self.workspace_id)
            .bind(slug)
            .bind(slug)
            .execute(&self.pool)
            .await
            .expect("puente");
        sqlx::query(
            "INSERT INTO bridge_versions (id, bridge_id, version, definition) \
             VALUES ($1, $2, 1, '{\"trigger\":{\"type\":\"http\"}}'::jsonb)",
        )
        .bind(version_id)
        .bind(bridge_id)
        .execute(&self.pool)
        .await
        .expect("versión");
        (bridge_id, version_id)
    }

    /// Ejecución con los pasos dados como (seq, estado, duración).
    async fn sembrar_ejecucion(
        &self,
        bridge_id: Uuid,
        version_id: Uuid,
        estado: &str,
        pasos: &[(i32, &str, Option<i32>)],
    ) -> Uuid {
        let execution_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO executions (id, workspace_id, bridge_id, bridge_version_id, \
             trigger_type, status, created_at, run_at) \
             VALUES ($1, $2, $3, $4, 'manual', $5, now(), now())",
        )
        .bind(execution_id)
        .bind(self.workspace_id)
        .bind(bridge_id)
        .bind(version_id)
        .bind(estado)
        .execute(&self.pool)
        .await
        .expect("ejecución");
        for (seq, status, duracion) in pasos {
            sqlx::query(
                "INSERT INTO execution_steps (execution_id, seq, block_instance_id, block_name, \
                 status, input, duration_ms) \
                 VALUES ($1, $2, $3, $3, $4, '{}'::jsonb, $5)",
            )
            .bind(execution_id)
            .bind(seq)
            .bind(format!("paso-{seq}"))
            .bind(status)
            .bind(duracion)
            .execute(&self.pool)
            .await
            .expect("paso");
        }
        execution_id
    }
}

#[tokio::test]
async fn el_listado_trae_el_resumen_de_pasos_en_orden_y_sin_io() {
    let Some(h) = begin().await else { return };
    let (puente, version) = h.sembrar_puente("pasos-a").await;
    let ok = h
        .sembrar_ejecucion(puente, version, "succeeded", &[
            (1, "succeeded", Some(120)),
            (2, "succeeded", Some(45)),
            (3, "succeeded", Some(210)),
        ])
        .await;
    let mala = h
        .sembrar_ejecucion(puente, version, "failed", &[
            (1, "succeeded", Some(90)),
            (2, "failed", Some(240)),
            (3, "skipped", None),
        ])
        .await;

    let respuesta = h.admin(Method::GET, "/api/v1/executions").await;
    assert_eq!(respuesta.status, StatusCode::OK);
    let pagina = respuesta.json();
    let items = pagina["items"].as_array().expect("items");

    let item = |id: &Uuid| {
        items
            .iter()
            .find(|i| i["id"] == json!(id.to_string()))
            .unwrap_or_else(|| panic!("la ejecución {id} no está en el listado"))
    };

    // Pasos de la correcta: en orden de cadena, con camelCase.
    let pasos_ok = item(&ok)["pasos"].as_array().expect("pasos de la ok");
    assert_eq!(
        pasos_ok,
        &vec![
            json!({"status": "succeeded", "durationMs": 120}),
            json!({"status": "succeeded", "durationMs": 45}),
            json!({"status": "succeeded", "durationMs": 210}),
        ]
    );

    // Pasos de la fallida: estados mezclados y skipped con duración nula.
    let pasos_mal = item(&mala)["pasos"].as_array().expect("pasos de la mala");
    assert_eq!(pasos_mal.len(), 3);
    assert_eq!(pasos_mal[1]["status"], json!("failed"));
    assert_eq!(pasos_mal[2]["status"], json!("skipped"));
    assert!(pasos_mal[2]["durationMs"].is_null());

    // Nada de IO en el resumen: solo status y durationMs.
    for paso in pasos_ok.iter().chain(pasos_mal.iter()) {
        let claves: Vec<&str> = paso.as_object().expect("objeto").keys().map(String::as_str).collect();
        assert_eq!(claves, vec!["status", "durationMs"], "el resumen no debe traer IO: {claves:?}");
    }
}

#[tokio::test]
async fn una_ejecucion_sin_pasos_lista_pasos_vacios() {
    let Some(h) = begin().await else { return };
    let (puente, version) = h.sembrar_puente("pasos-b").await;
    let en_cola = h.sembrar_ejecucion(puente, version, "pending", &[]).await;

    let respuesta = h.admin(Method::GET, "/api/v1/executions").await;
    assert_eq!(respuesta.status, StatusCode::OK);
    let pagina = respuesta.json();
    let items = pagina["items"].as_array().expect("items");
    let item = items
        .iter()
        .find(|i| i["id"] == json!(en_cola.to_string()))
        .expect("la ejecución en cola");
    assert!(item["pasos"].as_array().expect("pasos").is_empty());
}

#[tokio::test]
async fn el_resumen_no_cruza_workspaces() {
    let Some(h) = begin().await else { return };
    let (puente, version) = h.sembrar_puente("pasos-c").await;
    h.sembrar_ejecucion(puente, version, "succeeded", &[(1, "succeeded", Some(10))])
        .await;

    // Otro workspace con su propia ejecución de 2 pasos.
    let otro_workspace = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(otro_workspace)
        .bind("otro workspace pasos")
        .bind(format!("{WS_PREFIX}otro-{otro_workspace}"))
        .execute(&h.pool)
        .await
        .expect("workspace ajeno");
    let puente_ajeno = Uuid::now_v7();
    sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
        .bind(puente_ajeno)
        .bind(otro_workspace)
        .bind("pasos-ajeno")
        .bind("pasos-ajeno")
        .execute(&h.pool)
        .await
        .expect("puente ajeno");
    let ajena = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO executions (id, workspace_id, bridge_id, bridge_version_id, \
         trigger_type, status, created_at, run_at) \
         VALUES ($1, $2, $3, $3, 'manual', 'succeeded', now(), now())",
    )
    .bind(ajena)
    .bind(otro_workspace)
    .bind(puente_ajeno)
    .execute(&h.pool)
    .await
    .expect("ejecución ajena");
    sqlx::query(
        "INSERT INTO execution_steps (execution_id, seq, block_instance_id, block_name, \
         status, input, duration_ms) VALUES ($1, 1, 'x', 'x', 'succeeded', '{}'::jsonb, 5)",
    )
    .bind(ajena)
    .execute(&h.pool)
    .await
    .expect("paso ajeno");

    let respuesta = h.admin(Method::GET, "/api/v1/executions").await;
    assert_eq!(respuesta.status, StatusCode::OK);
    let pagina = respuesta.json();
    let items = pagina["items"].as_array().expect("items");
    assert!(!items.iter().any(|i| i["id"] == json!(ajena.to_string())));
    for item in items {
        let total: usize = item["pasos"].as_array().expect("pasos").len();
        assert!(total <= 3, "pasos de otro workspace filtrados al listado");
    }
}

#[tokio::test]
async fn el_listado_de_pasos_exige_autenticacion() {
    let Some(h) = begin().await else { return };
    let respuesta = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/executions")
                .body(Body::empty())
                .expect("petición"),
        )
        .await
        .expect("respuesta");
    assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);
}
