//! Tests de métricas agregadas (MVP5, task 1.1, spec `workspace-stats`).
//!
//! Ejecuciones sembradas por SQL con estados, días y duraciones controlados;
//! la agregación debe salir de UNA consulta (GROUPING SETS) y el rango
//! validarse antes de agregar. Contra Postgres **real**.
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
const WS_PREFIX: &str = "stats-test-";

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
            eprintln!("DATABASE_URL no está definida: se omiten los tests de métricas");
            return None;
        }
    };
    let pool = api::connect(&url)
        .await
        .expect("conexión a Postgres (¿corriste ./scripts/dev-postgres.sh?)");
    api::migrate(&pool).await.expect("migraciones aplicadas");

    let turno = DB_LOCK.lock().await;
    sqlx::query("DELETE FROM workspaces WHERE slug LIKE 'stats-test-%'")
        .execute(&pool)
        .await
        .expect("limpieza");

    let workspace_id = Uuid::now_v7();
    let slug = format!("{WS_PREFIX}{workspace_id}");
    let raw_key = format!("stats-test-key-{workspace_id}");
    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("workspace stats")
        .bind(&slug)
        .execute(&pool)
        .await
        .expect("workspace");
    sqlx::query("INSERT INTO api_keys (id, workspace_id, name, key_hash) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::now_v7())
        .bind(workspace_id)
        .bind("stats-test-key")
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
    async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> Resp {
        let mut builder = Request::builder().method(method).uri(uri);
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {}", self.key));
        let cuerpo = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let respuesta = self
            .app
            .clone()
            .oneshot(builder.body(cuerpo).expect("petición"))
            .await
            .expect("respuesta");
        let status = respuesta.status();
        let texto = String::from_utf8_lossy(
            &axum::body::to_bytes(respuesta.into_body(), 1 << 20)
                .await
                .expect("cuerpo"),
        )
        .into_owned();
        Resp {
            status,
            body: texto,
        }
    }
}

/// Siembra un puente con una versión 1 (para el FK de ejecuciones).
async fn sembrar_puente(h: &Harness, slug: &str) -> (Uuid, Uuid) {
    let bridge_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO bridges (id, workspace_id, slug, name) VALUES ($1, $2, $3, $4)")
        .bind(bridge_id)
        .bind(h.workspace_id)
        .bind(slug)
        .bind(format!("Puente {slug}"))
        .execute(&h.pool)
        .await
        .expect("bridge");
    sqlx::query(
        "INSERT INTO bridge_versions (id, bridge_id, version, definition) \
         VALUES ($1, $2, 1, '{\"trigger\":{\"type\":\"http\"}}'::jsonb)",
    )
    .bind(version_id)
    .bind(bridge_id)
    .execute(&h.pool)
    .await
    .expect("bridge_version");
    (bridge_id, version_id)
}

/// Siembra una ejecución con estado, antigüedad (días) y duración (ms).
#[allow(clippy::too_many_arguments)]
async fn sembrar_ejecucion(
    h: &Harness,
    bridge_id: Uuid,
    version_id: Uuid,
    status: &str,
    dias_atras: i64,
    duracion_ms: Option<i64>,
) {
    let created = Utc::now() - chrono::Duration::days(dias_atras);
    let started = duracion_ms.map(|_| created + chrono::Duration::milliseconds(10));
    let finished = duracion_ms.map(|ms| created + chrono::Duration::milliseconds(10 + ms));
    sqlx::query(
        "INSERT INTO executions (id, workspace_id, bridge_id, bridge_version_id, \
         trigger_type, status, created_at, started_at, finished_at) \
         VALUES ($1, $2, $3, $4, 'manual', $5, $6, $7, $8)",
    )
    .bind(Uuid::now_v7())
    .bind(h.workspace_id)
    .bind(bridge_id)
    .bind(version_id)
    .bind(status)
    .bind(created)
    .bind(started)
    .bind(finished)
    .execute(&h.pool)
    .await
    .expect("ejecución");
}

use chrono::Utc;

#[tokio::test]
async fn las_metricas_agregan_total_estados_serie_y_puente_de_un_rango() {
    let Some(h) = begin().await else { return };
    let (b1, v1) = sembrar_puente(&h, "stat-a").await;
    let (b2, v2) = sembrar_puente(&h, "stat-b").await;

    // b1: 3 exitosas (100/200/300 ms → media 200) + 1 fallida, hoy.
    sembrar_ejecucion(&h, b1, v1, "succeeded", 0, Some(100)).await;
    sembrar_ejecucion(&h, b1, v1, "succeeded", 0, Some(200)).await;
    sembrar_ejecucion(&h, b1, v1, "succeeded", 0, Some(300)).await;
    sembrar_ejecucion(&h, b1, v1, "failed", 0, Some(50)).await;
    // b2: 2 exitosas (400/400 ms), ayer + una pendiente sin started_at.
    sembrar_ejecucion(&h, b2, v2, "succeeded", 1, Some(400)).await;
    sembrar_ejecucion(&h, b2, v2, "succeeded", 1, Some(400)).await;
    sembrar_ejecucion(&h, b2, v2, "pending", 1, None).await;

    let from =
        (Utc::now() - chrono::Duration::days(5)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    // `to` con margen: las ejecuciones se siembran DESPUÉS de capturarlo.
    let to = (Utc::now() + chrono::Duration::hours(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let r = h
        .admin(
            Method::GET,
            &format!("/api/v1/stats?from={from}&to={to}"),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let stats = r.json();

    assert_eq!(stats["total"], json!(7), "{}", stats);
    assert_eq!(stats["byStatus"]["succeeded"], json!(5));
    assert_eq!(stats["byStatus"]["failed"], json!(1));
    assert_eq!(stats["byStatus"]["pending"], json!(1));
    // Media de las 5 exitosas: (100+200+300+400+400)/5 = 280.
    let media = stats["avgDurationMs"].as_f64().expect("media");
    assert!((media - 280.0).abs() < 1.0, "media: {media}");

    // Serie diaria: 2 días con ejecuciones (ayer 3, hoy 4).
    let serie = stats["daily"].as_array().expect("daily");
    assert_eq!(serie.len(), 2, "{}", stats);
    let ayer: i64 = serie.iter().map(|d| d["total"].as_i64().unwrap()).sum();
    assert_eq!(ayer, 7);
    let exitosas_serie: i64 = serie.iter().map(|d| d["succeeded"].as_i64().unwrap()).sum();
    assert_eq!(exitosas_serie, 5);

    // Desglose por puente: b1 (4) primero, b2 (3) después; medias por puente.
    let puentes = stats["bridges"]["items"].as_array().expect("bridges");
    assert_eq!(puentes.len(), 2);
    assert_eq!(puentes[0]["slug"], json!("stat-a"));
    assert_eq!(puentes[0]["total"], json!(4));
    assert_eq!(puentes[0]["succeeded"], json!(3));
    assert_eq!(puentes[0]["failed"], json!(1));
    let media_a = puentes[0]["avgDurationMs"].as_f64().unwrap();
    assert!((media_a - 200.0).abs() < 1.0, "media a: {media_a}");
    assert_eq!(puentes[1]["slug"], json!("stat-b"));
    assert!(stats["bridges"]["nextCursor"].is_null());
}

#[tokio::test]
async fn el_rango_invalido_se_rechaza_sin_agregar() {
    let Some(h) = begin().await else { return };
    let to = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // from > to
    let despues = (Utc::now() + chrono::Duration::hours(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let r = h
        .admin(
            Method::GET,
            &format!("/api/v1/stats?from={despues}&to={to}"),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);

    // Rango de 91 días
    let lejano = (Utc::now() - chrono::Duration::days(91))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let r = h
        .admin(
            Method::GET,
            &format!("/api/v1/stats?from={lejano}&to={to}"),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.body.contains("90"), "{}", r.body);

    // from no RFC 3339
    let r = h.admin(Method::GET, "/api/v1/stats?from=ayer", None).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.body.contains("RFC 3339"), "{}", r.body);
}

#[tokio::test]
async fn el_desglose_por_puente_se_pagina_por_cursor() {
    let Some(h) = begin().await else { return };
    let (b0, v0) = sembrar_puente(&h, "stat-pag-0").await;
    sembrar_ejecucion(&h, b0, v0, "succeeded", 0, Some(10)).await;

    // 51 puentes adicionales en una sola instrucción (todos con 1 exitosa).
    sqlx::query(
        "WITH numeros AS (SELECT generate_series(1, 51) AS n), \
         nuevos AS ( \
           INSERT INTO bridges (id, workspace_id, slug, name) \
           SELECT gen_random_uuid(), $1, 'stat-pag-' || n, 'Puente ' || n FROM numeros RETURNING id \
         ) \
         INSERT INTO bridge_versions (id, bridge_id, version, definition) \
         SELECT gen_random_uuid(), id, 1, '{}'::jsonb FROM nuevos",
    )
    .bind(h.workspace_id)
    .execute(&h.pool)
    .await
    .expect("51 puentes");

    // Ejecución para cada puente nuevo (vía JOIN con su versión 1).
    sqlx::query(
        "INSERT INTO executions (id, workspace_id, bridge_id, bridge_version_id, \
         trigger_type, status, created_at) \
         SELECT gen_random_uuid(), $1, b.id, bv.id, 'manual', 'succeeded', now() \
         FROM bridges b \
         JOIN bridge_versions bv ON bv.bridge_id = b.id AND bv.version = 1 \
         WHERE b.workspace_id = $1 AND b.slug LIKE 'stat-pag-%'",
    )
    .bind(h.workspace_id)
    .execute(&h.pool)
    .await
    .expect("ejecuciones paginadas");

    let r = h.admin(Method::GET, "/api/v1/stats", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let pagina1 = r.json();
    let items = pagina1["bridges"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 50, "primera página llena");
    let cursor = pagina1["bridges"]["nextCursor"]
        .as_str()
        .expect("nextCursor")
        .to_string();

    let r = h
        .admin(Method::GET, &format!("/api/v1/stats?cursor={cursor}"), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let pagina2 = r.json();
    let items2 = pagina2["bridges"]["items"].as_array().expect("items2");
    assert_eq!(items2.len(), 2, "51 puentes nuevos + stat-pag-0");
    assert!(pagina2["bridges"]["nextCursor"].is_null());
}

#[tokio::test]
async fn el_cursor_invalido_es_400() {
    let Some(h) = begin().await else { return };
    let r = h
        .admin(Method::GET, "/api/v1/stats?cursor=basura", None)
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
}
