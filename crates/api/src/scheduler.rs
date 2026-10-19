//! Scheduler de triggers cron (MVP2, spec `cron-triggers`, design.md D5).
//!
//! Job dentro del binario del api (instancia única en MVP2: con réplicas
//! haría falta líder; la cola SKIP LOCKED protege al worker, no al scheduler).
//! Cada tick, por cada puente con última versión publicada de trigger cron:
//!
//! 1. **Pre-encolado**: calcula la próxima ocurrencia tras el último instante
//!    **registrado** (el `scheduled_at` más reciente entre las ejecuciones
//!    cron del puente y las filas de `missed_triggers`) y la encola con
//!    `run_at` = instante programado, `trigger_type: 'cron'` y
//!    `trigger_data.scheduled_at`. El worker la reclama exactamente cuando
//!    vence (el índice parcial de la cola ordena por `run_at`).
//! 2. **Solape** (política fija skip): si la siguiente ocurrencia ya venció y
//!    la anterior sigue `pending | running`, no encola y registra
//!    `missed_triggers(reason: 'overlap')`.
//!
//! **Recuperación de caída** ([`recuperar_arranque`]): al arrancar, para cada
//! puente cron se detectan las ocurrencias vencidas no registradas desde el
//! último instante conocido; se encola **solo la más reciente** (la política
//! de solape manda si hay alguna ejecución viva) y el resto queda como
//! `missed_triggers(reason: 'downtime')`.
//!
//! Límites documentados: schedules ≥ 1 minuto (design.md) y tope de 10 000
//! ocurrencias por pase de recuperación (una caída de años no satura la BD).

// Módulo con `Result<_, Problem>` como todo el crate (mismo lint autorizado).
#![allow(clippy::result_large_err)]

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use domain::cron::{self, CronSchedule};
use serde_json::json;
use uuid::Uuid;

use crate::problem::{self, Problem};
use crate::state::AppState;

/// Intervalo del tick del scheduler.
pub const TICK: std::time::Duration = std::time::Duration::from_secs(1);

/// Tope de ocurrencias vencidas por pase de recuperación de caída.
const MAX_RECUPERACION: usize = 10_000;

/// Un puente cron activo: última versión publicada con trigger cron.
struct PuenteCron {
    bridge_id: Uuid,
    workspace_id: Uuid,
    version_id: Uuid,
    schedule: CronSchedule,
    /// Expresión textual original (para `trigger_data`).
    schedule_raw: String,
    timezone: Tz,
}

/// Arranca el scheduler: pase de recuperación de caída + tick por segundo.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        if let Err(error) = recuperar_arranque(&state, Utc::now()).await {
            tracing::warn!(?error, "scheduler: recuperación de arranque falló");
        }
        loop {
            if let Err(error) = tick(&state, Utc::now()).await {
                tracing::warn!(?error, "scheduler: tick con error");
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Un tick del scheduler sobre todos los puentes cron, con reloj inyectable.
/// Público para los tests: cada llamada procesa una oleada determinista.
pub async fn tick(state: &AppState, now: DateTime<Utc>) -> Result<usize, Problem> {
    let puentes = cargar_puentes_cron(state).await?;
    let mut acciones = 0;
    for puente in puentes {
        let last = last_known(state, puente.bridge_id).await?;
        match last {
            // Primera programación del puente: la próxima ocurrencia futura,
            // pre-encolada con su run_at (el worker la reclama al vencer).
            None => {
                let from = now.with_timezone(&puente.timezone);
                if let Some(candidate) = puente.schedule.next_after(&from) {
                    encolar(state, &puente, candidate.with_timezone(&Utc)).await?;
                    acciones += 1;
                }
            }
            Some(last) => {
                let from = last.with_timezone(&puente.timezone);
                if let Some(candidate) = puente.schedule.next_after(&from) {
                    let candidate = candidate.with_timezone(&Utc);
                    let unresolved = hay_unresolved(state, puente.bridge_id).await?;
                    if candidate <= now {
                        // La siguiente ocurrencia ya venció sin registrarse:
                        // solape si la anterior sigue viva, si no, encolar
                        // (el worker la reclama al instante).
                        if unresolved {
                            registrar_missed(state, puente.bridge_id, candidate, "overlap").await?;
                        } else {
                            encolar(state, &puente, candidate).await?;
                        }
                        acciones += 1;
                    } else if !unresolved {
                        // D5: la próxima ocurrencia futura queda PRE-ENCOLADA
                        // con su run_at; el worker la reclama cuando vence.
                        // Con una ejecución viva no se pre-encola nada: el
                        // solape decidirá cuando la ocurrencia venza.
                        encolar(state, &puente, candidate).await?;
                        acciones += 1;
                    }
                }
            }
        }
    }
    Ok(acciones)
}

/// Pase de arranque: detecta las ocurrencias vencidas durante una caída,
/// encola solo la más reciente y registra el resto como `downtime`.
/// Devuelve el número de filas escritas (encolados + missed).
pub async fn recuperar_arranque(state: &AppState, now: DateTime<Utc>) -> Result<usize, Problem> {
    let puentes = cargar_puentes_cron(state).await?;
    let mut escritas = 0;
    for puente in puentes {
        let Some(last) = last_known(state, puente.bridge_id).await? else {
            // Sin historial: el primer tick programa la próxima futura.
            continue;
        };
        let from = last.with_timezone(&puente.timezone);
        let mut vencidas: Vec<DateTime<Utc>> = Vec::new();
        let mut cursor = from;
        while let Some(candidate) = puente.schedule.next_after(&cursor) {
            let candidate = candidate.with_timezone(&Utc);
            if candidate > now || vencidas.len() >= MAX_RECUPERACION {
                break;
            }
            vencidas.push(candidate);
            cursor = candidate.with_timezone(&puente.timezone);
        }
        if vencidas.is_empty() {
            continue;
        }
        // La más reciente se encola (salvo solape); el resto, downtime.
        for vencida in &vencidas[..vencidas.len() - 1] {
            registrar_missed(state, puente.bridge_id, *vencida, "downtime").await?;
            escritas += 1;
        }
        let ultima = vencidas[vencidas.len() - 1];
        if hay_unresolved(state, puente.bridge_id).await? {
            registrar_missed(state, puente.bridge_id, ultima, "overlap").await?;
        } else {
            encolar(state, &puente, ultima).await?;
        }
        escritas += 1;
    }
    Ok(escritas)
}

/// Carga los puentes con trigger cron en su última versión publicada.
async fn cargar_puentes_cron(state: &AppState) -> Result<Vec<PuenteCron>, Problem> {
    type FilaCron = (Uuid, Uuid, Uuid, Option<String>, Option<String>);
    let filas: Vec<FilaCron> = sqlx::query_as(
        "SELECT b.id, b.workspace_id, bv.id, \
                bv.definition->'trigger'->>'schedule', \
                bv.definition->'trigger'->>'timezone' \
         FROM bridges b \
         JOIN bridge_versions bv ON bv.bridge_id = b.id AND bv.version = b.latest_version \
         WHERE b.latest_version IS NOT NULL \
           AND bv.definition->'trigger'->>'type' = 'cron'",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("scheduler: cargar los puentes cron: {error}")))?;

    let mut puentes = Vec::with_capacity(filas.len());
    for (bridge_id, workspace_id, version_id, schedule, timezone) in filas {
        let (Some(schedule), Some(timezone)) = (schedule, timezone) else {
            // La publicación valida schedule y timezone; un snapshot viejo o
            // manipulado sin ellos se ignora (y se deja constancia en el log).
            tracing::warn!(%bridge_id, "scheduler: trigger cron sin schedule/timezone; ignorado");
            continue;
        };
        let Ok(parsed) = cron::parse(&schedule) else {
            tracing::warn!(%bridge_id, %schedule, "scheduler: schedule inválido en el snapshot");
            continue;
        };
        let Ok(tz) = timezone.parse::<Tz>() else {
            tracing::warn!(%bridge_id, %timezone, "scheduler: timezone inválida en el snapshot");
            continue;
        };
        puentes.push(PuenteCron {
            bridge_id,
            workspace_id,
            version_id,
            schedule: parsed,
            schedule_raw: schedule,
            timezone: tz,
        });
    }
    Ok(puentes)
}

/// Último instante de ocurrencia registrado para el puente: el máximo entre
/// los `scheduled_at` de sus ejecuciones cron y sus `missed_triggers`.
async fn last_known(state: &AppState, bridge_id: Uuid) -> Result<Option<DateTime<Utc>>, Problem> {
    let row: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT max(scheduled_at) FROM ( \
             SELECT (trigger_data->>'scheduled_at')::timestamptz AS scheduled_at \
               FROM executions WHERE bridge_id = $1 AND trigger_type = 'cron' \
             UNION ALL \
             SELECT scheduled_at FROM missed_triggers WHERE bridge_id = $1 \
         ) registrados",
    )
    .bind(bridge_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("scheduler: último instante: {error}")))?;
    Ok(row)
}

/// `true` si el puente tiene una ejecución cron `pending | running`.
async fn hay_unresolved(state: &AppState, bridge_id: Uuid) -> Result<bool, Problem> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM executions \
         WHERE bridge_id = $1 AND trigger_type = 'cron' AND status IN ('pending', 'running')",
    )
    .bind(bridge_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("scheduler: solape: {error}")))?;
    Ok(count > 0)
}

/// Pre-encola la ocurrencia: fila `pending` con `run_at` = instante
/// programado y `trigger_data.scheduled_at` (D5).
async fn encolar(
    state: &AppState,
    puente: &PuenteCron,
    scheduled_at: DateTime<Utc>,
) -> Result<(), Problem> {
    sqlx::query(
        "INSERT INTO executions \
             (id, workspace_id, bridge_id, bridge_version_id, trigger_type, trigger_data, \
              trigger_principal, status, run_at) \
         VALUES ($1, $2, $3, $4, 'cron', $5, $6, 'pending', $7)",
    )
    .bind(Uuid::now_v7())
    .bind(puente.workspace_id)
    .bind(puente.bridge_id)
    .bind(puente.version_id)
    .bind(json!({
        "scheduled_at": scheduled_at.to_rfc3339(),
        "schedule": puente.schedule_text(),
        "timezone": puente.timezone.name(),
    }))
    .bind(json!({ "type": "system", "motivo": "cron" }))
    .bind(scheduled_at)
    .execute(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("scheduler: encolar la ocurrencia: {error}")))?;
    Ok(())
}

/// Registra una ocurrencia no ejecutada en `missed_triggers`.
async fn registrar_missed(
    state: &AppState,
    bridge_id: Uuid,
    scheduled_at: DateTime<Utc>,
    reason: &str,
) -> Result<(), Problem> {
    sqlx::query(
        "INSERT INTO missed_triggers (bridge_id, scheduled_at, reason) VALUES ($1, $2, $3)",
    )
    .bind(bridge_id)
    .bind(scheduled_at)
    .bind(reason)
    .execute(&state.pool)
    .await
    .map_err(|error| problem::internal(format!("scheduler: registrar el missed: {error}")))?;
    Ok(())
}

impl PuenteCron {
    /// Expresión cron textual (para `trigger_data`).
    fn schedule_text(&self) -> String {
        // La expresión original vive en el snapshot; la reconstruimos del
        // parser no es posible (la máscara no es textual), así que el caller
        // guarda la que trae la definición. Este campo se llena en
        // `cargar_puentes_cron` vía `schedule_raw`.
        self.schedule_raw.clone()
    }
}
