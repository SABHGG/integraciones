//! Persistencia de ejecuciones: `executions` + `execution_steps`.
//!
//! La tabla `executions` **es** la cola (design.md): la fila nace `pending` en
//! un solo commit, el worker la reclama con `FOR UPDATE SKIP LOCKED` pasándola a
//! `running` y el cierre escribe **en una única transacción** todos los
//! `execution_steps` más el estado final `succeeded | failed` con su error.

use chrono::{DateTime, Utc};
use domain::definition::TriggerType;
use domain::execution::ExecutionStatus;
use serde_json::Value as Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::executor::ExecutionOutcome;

/// Columnas de `executions` tal como las lee el store.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ExecutionRow {
    /// Id de la ejecución (UUID v7 generado por la aplicación).
    pub id: Uuid,
    /// Workspace dueño del puente.
    pub workspace_id: Uuid,
    /// Puente disparado.
    pub bridge_id: Uuid,
    /// Versión exacta de la definición con la que corre.
    pub bridge_version_id: Uuid,
    /// Tipo de disparador (`http`, `cron`, `event`, `manual`).
    pub trigger_type: String,
    /// Datos del trigger (`$.trigger`).
    pub trigger_data: Json,
    /// Estado: `pending | running | succeeded | failed`.
    pub status: String,
    /// Clave de idempotencia del disparo, si la hubo.
    pub idempotency_key: Option<String>,
    /// Error estructurado del cierre, si falló.
    pub error: Option<Json>,
    /// Momento a partir del cual la ejecución es reclamable.
    pub run_at: DateTime<Utc>,
    /// Momento del claim (inicio real).
    pub started_at: Option<DateTime<Utc>>,
    /// Momento del cierre.
    pub finished_at: Option<DateTime<Utc>>,
    /// Alta de la fila.
    pub created_at: DateTime<Utc>,
    /// Última modificación.
    pub updated_at: DateTime<Utc>,
}

impl ExecutionRow {
    /// Estado tipado de la fila.
    pub fn status_typed(&self) -> Option<ExecutionStatus> {
        ExecutionStatus::parse(&self.status)
    }
}

/// Datos para crear una ejecución nueva (nace `pending`).
#[derive(Debug, Clone)]
pub struct NewExecution {
    /// Workspace dueño del puente.
    pub workspace_id: Uuid,
    /// Puente disparado.
    pub bridge_id: Uuid,
    /// Versión de la definición con la que correrá.
    pub bridge_version_id: Uuid,
    /// Tipo de disparador.
    pub trigger_type: TriggerType,
    /// Datos del trigger.
    pub trigger_data: Json,
    /// Clave de idempotencia opcional del disparo.
    pub idempotency_key: Option<String>,
}

/// Error del store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Fallo de base de datos (incluye violaciones de constraints).
    #[error("error de base de datos: {0}")]
    Database(#[from] sqlx::Error),
    /// No se pudo serializar el error de la ejecución a JSONB.
    #[error("no se pudo serializar el error de la ejecución: {0}")]
    Serialize(#[from] serde_json::Error),
    /// Se intentó cerrar una ejecución que no está en estado `running`.
    #[error("la ejecución {id} no está en estado running: no se puede cerrar")]
    NotRunning {
        /// Ejecución que se intentó cerrar.
        id: Uuid,
    },
}

/// Acceso a la cola y al trace de ejecuciones.
#[derive(Debug, Clone)]
pub struct ExecutionStore {
    pool: PgPool,
}

impl ExecutionStore {
    /// Crea el store sobre un pool de Postgres.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Pool subyacente (para consultas de la API).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Crea la ejecución en estado `pending` con un UUID v7 generado por la
    /// aplicación. La fila nace atómicamente: no hay ejecuciones fantasma.
    pub async fn create(&self, new: NewExecution) -> Result<ExecutionRow, StoreError> {
        // UUID v7: ordenable temporalmente, sirve como executionId público.
        let id = Uuid::now_v7();
        let sql = format!(
            "INSERT INTO executions
                 (id, workspace_id, bridge_id, bridge_version_id, trigger_type,
                  trigger_data, idempotency_key, status)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending')
             RETURNING {EXECUTION_COLUMNS}"
        );

        let row = sqlx::query_as::<_, ExecutionRow>(&sql)
            .bind(id)
            .bind(new.workspace_id)
            .bind(new.bridge_id)
            .bind(new.bridge_version_id)
            .bind(new.trigger_type.as_str())
            .bind(new.trigger_data)
            .bind(new.idempotency_key)
            .fetch_one(&self.pool)
            .await?;

        Ok(row)
    }

    /// Reclama la siguiente ejecución pendiente y la pasa a `running`.
    ///
    /// `FOR UPDATE SKIP LOCKED`: dos workers nunca reciben la misma fila.
    pub async fn claim_next(&self) -> Result<Option<ExecutionRow>, StoreError> {
        let sql = format!(
            "UPDATE executions
                SET status = 'running', started_at = now(), updated_at = now()
              WHERE id = (
                    SELECT id FROM executions
                     WHERE status = 'pending' AND run_at <= now()
                     ORDER BY run_at, created_at
                     FOR UPDATE SKIP LOCKED
                     LIMIT 1
              )
              RETURNING {EXECUTION_COLUMNS}"
        );

        let row = sqlx::query_as::<_, ExecutionRow>(&sql)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row)
    }

    /// Cierra la ejecución: escribe todos los pasos y el estado final en una
    /// única transacción. El error estructurado se persiste con todo su detalle.
    pub async fn finish(
        &self,
        execution_id: Uuid,
        outcome: &ExecutionOutcome,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;

        let error = outcome
            .error
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;

        let updated = sqlx::query(
            "UPDATE executions
                SET status = $2, error = $3, finished_at = now(), updated_at = now()
              WHERE id = $1 AND status = 'running'",
        )
        .bind(execution_id)
        .bind(outcome.status.as_str())
        .bind(error)
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            // El `DROP` del transaction hace rollback: no queda nada escrito.
            return Err(StoreError::NotRunning { id: execution_id });
        }

        for step in &outcome.steps {
            let step_error = step.error.as_ref().map(serde_json::to_value).transpose()?;

            sqlx::query(
                "INSERT INTO execution_steps
                     (execution_id, seq, block_instance_id, block_name, status,
                      input, output, error, duration_ms)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(execution_id)
            .bind(step.seq)
            .bind(&step.block_instance_id)
            .bind(&step.block_name)
            .bind(step.status.as_str())
            .bind(&step.input)
            .bind(step.output.as_ref())
            .bind(step_error)
            .bind(step.duration_ms)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

/// Columnas de `executions` que devuelve el store en `RETURNING`.
const EXECUTION_COLUMNS: &str = "id, workspace_id, bridge_id, bridge_version_id, trigger_type, \
     trigger_data, status, idempotency_key, error, run_at, started_at, finished_at, \
     created_at, updated_at";
