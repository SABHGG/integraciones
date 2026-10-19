# Execution Store Specification

## Purpose

Almacén de ejecuciones observable: la propia tabla `executions` actúa como cola (SKIP LOCKED) y registro, con IO completo por bloque en `execution_steps`, duración, error y retención configurable (30 días de IO, 1 año de resúmenes). Base de la auditoría y del replay futuro.

## Requirements

### Requirement: Registro completo de ejecuciones
El sistema SHALL persistir cada ejecución con: referencia al puente y a la versión exacta de la definición con la que corrió, tipo de disparador, datos de entrada del trigger, estado (pending → running → succeeded|failed), fechas y error estructurado. `trigger_data` SHALL registrar además el origen del disparo: topic y payload del evento, ejecución original en un replay, o instante programado en un cron.

#### Scenario: Toda ejecución es interpretable a posteriori
- **WHEN** se consulta una ejecución histórica `GET /api/v1/executions/{id}`
- **THEN** se obtienen su estado, fechas, entrada del trigger y la referencia a la versión de la definición con la que corrió

#### Scenario: El origen del disparo es rastreable
- **WHEN** se consulta una ejecución disparada por evento o creada por replay
- **THEN** `trigger_data` identifica el topic y payload del evento, o la ejecución original del replay, respectivamente

### Requirement: IO completo por bloque con duración y error
El sistema SHALL persistir en `execution_steps` la entrada resuelta, la salida, el error (si lo hubo) y la duración en ms de cada bloque, en secuencia (`execution_id, seq`), en la misma transacción que el avance de estado. Los pasos no ejecutados por enrutado condicional SHALL registrarse como `skipped` (sin input, salida ni duración); los campos cubiertos por `mask` SHALL persistirse ya enmascarados (`***`), de forma irreversible.

#### Scenario: Trace consultable por bloque
- **WHEN** se solicita `GET /api/v1/executions/{id}/trace`
- **THEN** se obtienen los pasos en orden con IO completo, duración por bloque y el punto exacto del fallo si existe

#### Scenario: Los pasos condicionales no tomados quedan en el trace
- **WHEN** un `switch` enruta por la ruta `vip` dejando la ruta `standard` sin ejecutar
- **THEN** el trace muestra el paso de `standard` como `skipped`, en su `seq` original y sin IO

#### Scenario: El campo enmascarado se persiste enmascarado
- **WHEN** la versión del puente declara `mask` sobre un campo que un bloque produce
- **THEN** `execution_steps` guarda `***` en ese campo y el valor original no existe en ninguna tabla

### Requirement: Cola de ejecución sobre la propia tabla
La tabla `executions` SHALL hacer de cola de trabajo: filas `status='pending'` con `run_at` e índice parcial, consumidas con `FOR UPDATE SKIP LOCKED`. La ejecución y su fila SHALL nacer atómicamente en una transacción.

#### Scenario: Sin filas huérfanas ni ejecuciones fantasma
- **WHEN** se crea una ejecución por invocación HTTP
- **THEN** la fila nace en un solo commit (pending) y el worker la reclama con SKIP LOCKED sin duplicados entre workers

### Requirement: Retención configurable
El sistema SHALL purgar por job el IO de `execution_steps` a 30 días y los resúmenes de `executions` a 1 año, con valores configurables globalmente.

#### Scenario: El IO viejo se purga sin romper el resumen
- **WHEN** el job de retención corre sobre ejecuciones de más de 30 días
- **THEN** se eliminan sus `execution_steps` y el registro `executions` permanece consultable hasta su purga anual

### Requirement: Registro de disparos perdidos y entregas webhook
El sistema SHALL persistir las ocurrencias de cron no ejecutadas en `missed_triggers` (`bridge_id`, `scheduled_at`, `reason: overlap|downtime`) y el ciclo de vida de cada entrega de resultado en `webhook_deliveries` (`execution_id`, `url`, `status: pending|delivered|failed`, `attempts`, `response_status`, `next_retry_at`, `last_attempt_at`), consultables por el plano de administración.

#### Scenario: Un solapamiento queda registrado
- **WHEN** el scheduler salta una ocurrencia porque la anterior sigue corriendo
- **THEN** `missed_triggers` contiene la fila con `scheduled_at` de la ocurrencia y `reason: "overlap"`

#### Scenario: El estado de las entregas es consultable
- **WHEN** se consulta la ejecución que disparó un webhook tras dos intentos fallidos
- **THEN** su entrega figura `pending` con `attempts: 2`, el último `response_status` y `next_retry_at` programado
