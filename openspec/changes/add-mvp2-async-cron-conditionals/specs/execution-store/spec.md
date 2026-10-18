# Spec Delta: execution-store

## MODIFIED Requirements

### Requirement: IO completo por bloque con duración y error
El sistema SHALL persistir en `execution_steps` la entrada resuelta, la salida, el error (si lo hubo) y la duración en ms de cada bloque, en secuencia (`execution_id, seq`), en la misma transacción que el avance de estado de la ejecución. El estado del paso SHALL admitir `succeeded`, `failed` y `skipped`: los pasos no ejecutados por enrutado condicional (`when` falso o ruta de `switch` no tomada) SHALL registrarse como `skipped`, sin input ejecutado, salida ni duración.

#### Scenario: Trace consultable por bloque
- **WHEN** se solicita `GET /api/v1/executions/{id}/trace`
- **THEN** se obtienen los pasos en orden con IO completo, duración por bloque y el punto exacto del fallo si existe

#### Scenario: Los pasos condicionales no tomados quedan en el trace
- **WHEN** un `switch` enruta por la ruta `vip` dejando la ruta `standard` sin ejecutar
- **THEN** el trace muestra el paso de `standard` como `skipped`, en su `seq` original y sin IO

## ADDED Requirements

### Requirement: Registro de disparos perdidos y entregas webhook
El sistema SHALL persistir las ocurrencias de cron no ejecutadas en `missed_triggers` (`bridge_id`, `scheduled_at`, `reason: overlap|downtime`) y el ciclo de vida de cada entrega de resultado en `webhook_deliveries` (`execution_id`, `url`, `status: pending|delivered|failed`, `attempts`, `response_status`, `next_retry_at`, `last_attempt_at`), consultables por el plano de administración.

#### Scenario: Un solapamiento queda registrado
- **WHEN** el scheduler salta una ocurrencia porque la anterior sigue corriendo
- **THEN** `missed_triggers` contiene la fila con `scheduled_at` de la ocurrencia y `reason: "overlap"`

#### Scenario: El estado de las entregas es consultable
- **WHEN** se consulta la ejecución que disparó un webhook tras dos intentos fallidos
- **THEN** su entrega figura `pending` con `attempts: 2`, el último `response_status` y `next_retry_at` programado
