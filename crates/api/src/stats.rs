//! Métricas agregadas del workspace (MVP5, spec `workspace-stats`).
//!
//! `GET /api/v1/stats{?from,to}` — **una sola consulta** agregada sobre
//! `executions` (GROUPING SETS: serie diaria, estados, desglose por puente
//! y totales; sin N+1). El rango se valida antes de agregar: RFC 3339,
//! `from` ≤ `to` y máximo 90 días. Solo lectura: la matriz de roles la
//! deja al alcance de `viewer`.
#![allow(clippy::result_large_err)]
use axum::Extension;
use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::auth::AuthedKey;
use crate::cursor;
use crate::problem::{self, Problem};
use crate::state::AppState;

/// Rango máximo de agregación (design D1): acota el coste de la consulta.
const RANGO_MAXIMO_DIAS: i64 = 90;
/// Página del desglose por puente.
const LIMITE_PUENTES: usize = 50;

#[derive(Debug, Deserialize)]
pub struct RangoQuery {
    from: Option<String>,
    to: Option<String>,
    cursor: Option<String>,
}

/// Una fila del GROUPING SETS: según el conjunto, rellena `dia`/`estado`
/// o `puente`/`slug`, o nada (fila de totales).
#[derive(Debug, sqlx::FromRow)]
struct FilaAgregada {
    dia: Option<chrono::NaiveDate>,
    estado: Option<String>,
    bridge_id: Option<Uuid>,
    slug: Option<String>,
    total: i64,
    exitosas: i64,
    fallidas: i64,
    avg_ms: Option<f64>,
}

/// Serie diaria (solo días con ejecuciones; la UI rellena los huecos).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiaDto {
    day: String,
    total: i64,
    succeeded: i64,
    failed: i64,
}

/// Desglose por puente del mismo rango.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PuenteStatsDto {
    bridge_id: Uuid,
    slug: String,
    total: i64,
    succeeded: i64,
    failed: i64,
    avg_duration_ms: Option<f64>,
}

/// Respuesta completa de `GET /stats`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsDto {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    total: i64,
    by_status: Value,
    avg_duration_ms: Option<f64>,
    daily: Vec<DiaDto>,
    bridges: PagePuentes,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PagePuentes {
    items: Vec<PuenteStatsDto>,
    next_cursor: Option<String>,
}

/// Límites del rango: RFC 3339 válido, `from` ≤ `to`, máximo 90 días.
fn resolver_rango(query: &RangoQuery) -> Result<(DateTime<Utc>, DateTime<Utc>), Problem> {
    let to = match &query.to {
        Some(raw) => DateTime::parse_from_rfc3339(raw)
            .map_err(|_| problem::bad_request("`to` no es una fecha RFC 3339 válida"))?
            .with_timezone(&Utc),
        None => Utc::now(),
    };
    let from = match &query.from {
        Some(raw) => DateTime::parse_from_rfc3339(raw)
            .map_err(|_| problem::bad_request("`from` no es una fecha RFC 3339 válida"))?
            .with_timezone(&Utc),
        None => to - Duration::days(30),
    };
    if from > to {
        return Err(problem::bad_request(
            "`from` debe ser anterior o igual a `to`",
        ));
    }
    if to - from > Duration::days(RANGO_MAXIMO_DIAS) {
        return Err(problem::bad_request(format!(
            "el rango no puede superar los {RANGO_MAXIMO_DIAS} días"
        )));
    }
    Ok((from, to))
}

/// GET /api/v1/stats — agregación única (design D1).
pub async fn obtener_stats(
    State(state): State<AppState>,
    Extension(key): Extension<AuthedKey>,
    Query(query): Query<RangoQuery>,
) -> Result<Json<StatsDto>, Problem> {
    let workspace_id = key.workspace()?;
    let (from, to) = resolver_rango(&query)?;

    // Una sola consulta: cuatro conjuntos de agrupación plegados en Rust.
    let filas = sqlx::query_as::<_, FilaAgregada>(
        "SELECT \
           (DATE(date_trunc('day', e.created_at))) AS dia, \
           e.status AS estado, \
           b.id AS bridge_id, \
           b.slug AS slug, \
           count(*) AS total, \
           count(*) FILTER (WHERE e.status = 'succeeded') AS exitosas, \
           count(*) FILTER (WHERE e.status = 'failed') AS fallidas, \
           avg((EXTRACT(EPOCH FROM (e.finished_at - e.started_at)) * 1000)::float8) \
             FILTER (WHERE e.status = 'succeeded' \
                     AND e.started_at IS NOT NULL AND e.finished_at IS NOT NULL) AS avg_ms \
         FROM executions e \
         JOIN bridges b ON b.id = e.bridge_id \
         WHERE e.workspace_id = $1 AND e.created_at >= $2 AND e.created_at < $3 \
         GROUP BY GROUPING SETS ((date_trunc('day', e.created_at), e.status), (e.status), (b.id, b.slug), ())",
    )
    .bind(workspace_id)
    .bind(from)
    .bind(to)
    .fetch_all(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("agregar las métricas: {error}")))?;

    plegar(filas, from, to, &query.cursor)
}

/// Plega las filas del GROUPING SETS en la respuesta (paginación en
/// memoria del desglose: la consulta sigue siendo una).
fn plegar(
    filas: Vec<FilaAgregada>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    cursor_crudo: &Option<String>,
) -> Result<Json<StatsDto>, Problem> {
    let mut total = 0i64;
    let mut avg_global: Option<f64> = None;
    let mut by_status = serde_json::Map::new();
    let mut diarias: std::collections::BTreeMap<String, DiaDto> = Default::default();
    let mut puentes: Vec<PuenteStatsDto> = Vec::new();

    for fila in filas {
        if let Some(bridge_id) = fila.bridge_id {
            // Conjunto (bridge_id, slug): desglose por puente.
            puentes.push(PuenteStatsDto {
                bridge_id,
                slug: fila.slug.unwrap_or_default(),
                total: fila.total,
                succeeded: fila.exitosas,
                failed: fila.fallidas,
                avg_duration_ms: fila.avg_ms,
            });
        } else if let Some(dia) = fila.dia {
            // Conjunto (día, estado): serie diaria.
            let entrada = diarias.entry(dia.to_string()).or_insert_with(|| DiaDto {
                day: dia.to_string(),
                total: 0,
                succeeded: 0,
                failed: 0,
            });
            entrada.total += fila.total;
            if fila.estado.as_deref() == Some("succeeded") {
                entrada.succeeded += fila.total;
            }
            if fila.estado.as_deref() == Some("failed") {
                entrada.failed += fila.total;
            }
        } else if let Some(estado) = fila.estado {
            // Conjunto (estado): conteo global por estado.
            by_status.insert(estado, Value::from(fila.total));
        } else {
            // Conjunto (): totales globales.
            total = fila.total;
            avg_global = fila.avg_ms;
        }
    }

    // Desglose por puente ordenado por impacto y paginado por cursor
    // (offset codificado con el mismo esquema de cursores del plano).
    puentes.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.slug.cmp(&b.slug)));
    let offset = match cursor_crudo {
        Some(raw) => {
            let partes = cursor::decode(raw, 1)
                .map_err(|_| problem::bad_request("el cursor es inválido"))?;
            partes[0]
                .parse::<usize>()
                .map_err(|_| problem::bad_request("el cursor es inválido"))?
        }
        None => 0,
    };
    let fin = (offset + LIMITE_PUENTES).min(puentes.len());
    let items = puentes
        .get(offset..fin)
        .map(<[PuenteStatsDto]>::to_vec)
        .unwrap_or_default();
    let next_cursor = if puentes.len() > fin {
        Some(cursor::encode(&[&fin.to_string()]))
    } else {
        None
    };

    Ok(Json(StatsDto {
        from,
        to,
        total,
        by_status: Value::Object(by_status),
        avg_duration_ms: avg_global,
        daily: diarias.into_values().collect(),
        bridges: PagePuentes { items, next_cursor },
    }))
}

// ---------------------------------------------------------------- Router ---

/// Rutas de métricas bajo `/api/v1`.
pub fn router() -> axum::routing::Router<AppState> {
    axum::routing::Router::new().route("/stats", axum::routing::get(obtener_stats))
}
