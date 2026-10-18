//! Job de retención (task 5.3): purga por `created_at` del IO viejo y de los
//! resúmenes antiguos de ejecuciones.
//!
//! El spec `execution-store` exige purgar el IO de `execution_steps` a los
//! 30 días y los resúmenes de `executions` a 1 año, ambos configurables
//! globalmente: la eliminación del IO viejo no rompe el resumen, que sigue
//! consultable hasta su propia ventana.
//!
//! `purge` es la operación pura (testable); `spawn` la envuelve en un job de
//! fondo con intervalo configurable (`RETENTION_INTERVAL_SECS`, por defecto
//! un día) que el servidor arranca al levantarse.

use sqlx::PgPool;

/// Ventanas de retención (en días) e intervalo del job (en segundos).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Edad máxima del IO de `execution_steps` (30 días por defecto).
    pub steps_days: i64,
    /// Edad máxima de los resúmenes de `executions` (1 año por defecto).
    pub executions_days: i64,
    /// Cada cuánto corre el job de fondo (86400 s = un día).
    pub interval_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            steps_days: 30,
            executions_days: 365,
            interval_secs: 86_400,
        }
    }
}

impl Config {
    /// Lee la configuración del entorno con los defaults del spec.
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Variante inyectable del lector de entorno (testable sin estado global).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let dias = |name: &str, default: i64| {
            lookup(name)
                .and_then(|raw| raw.parse::<i64>().ok())
                .filter(|days| *days >= 0)
                .unwrap_or(default)
        };
        Self {
            steps_days: dias("RETENTION_STEPS_DAYS", 30),
            executions_days: dias("RETENTION_EXECUTIONS_DAYS", 365),
            interval_secs: lookup("RETENTION_INTERVAL_SECS")
                .and_then(|raw| raw.parse::<u64>().ok())
                .filter(|secs| *secs > 0)
                .unwrap_or(86_400),
        }
    }
}

/// Filas eliminadas por una pasada de purga.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeReport {
    /// Filas de `execution_steps` eliminadas.
    pub steps: u64,
    /// Filas de `executions` eliminadas.
    pub executions: u64,
}

/// Purga el IO de `execution_steps` mayor de `steps_days` y los resúmenes de
/// `executions` mayores de `executions_days`.
///
/// El orden importa: primero el IO (ventana corta) y después los resúmenes
/// (ventana larga), de modo que un resumen nunca queda sin sus pasos dentro
/// de la ventana de 30 días.
pub async fn purge(
    pool: &PgPool,
    steps_days: i64,
    executions_days: i64,
) -> Result<PurgeReport, sqlx::Error> {
    let steps = sqlx::query(
        "DELETE FROM execution_steps WHERE created_at < now() - make_interval(days => $1::int)",
    )
    .bind(steps_days)
    .execute(pool)
    .await?
    .rows_affected();

    let executions = sqlx::query(
        "DELETE FROM executions WHERE created_at < now() - make_interval(days => $1::int)",
    )
    .bind(executions_days)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(PurgeReport { steps, executions })
}

/// Arranca el job de fondo: una pasada inmediata y luego cada
/// `config.interval_secs`. Los errores se registran y no tumban el bucle:
/// la siguiente pasada reintentará.
pub fn spawn(pool: PgPool, config: Config) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match purge(&pool, config.steps_days, config.executions_days).await {
                Ok(reporte) if reporte.steps > 0 || reporte.executions > 0 => {
                    tracing::info!(
                        steps = reporte.steps,
                        executions = reporte.executions,
                        "retención: purga aplicada"
                    );
                }
                Ok(_) => tracing::debug!("retención: nada que purgar"),
                Err(error) => tracing::error!(%error, "retención: la purga falló"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(config.interval_secs)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_defaults_son_los_del_spec() {
        let config = Config::default();
        assert_eq!(config.steps_days, 30);
        assert_eq!(config.executions_days, 365);
        assert_eq!(config.interval_secs, 86_400);
    }

    #[test]
    fn from_lookup_lee_las_tres_variables_con_defaults_frente_a_basura() {
        // Valores válidos.
        let env = [
            ("RETENTION_STEPS_DAYS", "7"),
            ("RETENTION_EXECUTIONS_DAYS", "90"),
            ("RETENTION_INTERVAL_SECS", "3600"),
        ];
        let config = Config::from_lookup(|name| {
            env.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        });
        assert_eq!(config.steps_days, 7);
        assert_eq!(config.executions_days, 90);
        assert_eq!(config.interval_secs, 3600);

        // Basura o negativos: cae al default.
        let env = [
            ("RETENTION_STEPS_DAYS", "treinta"),
            ("RETENTION_EXECUTIONS_DAYS", "-5"),
            ("RETENTION_INTERVAL_SECS", "0"),
        ];
        let config = Config::from_lookup(|name| {
            env.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        });
        assert_eq!(config.steps_days, 30);
        assert_eq!(config.executions_days, 365);
        assert_eq!(config.interval_secs, 86_400);
    }
}
