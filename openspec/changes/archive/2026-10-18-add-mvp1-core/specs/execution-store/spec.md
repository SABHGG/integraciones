# Execution Store

## ADDED Requirements

### Requirement: Registro completo de ejecuciones
El sistema SHALL persistir cada ejecución con: referencia al puente y a la versión exacta de la definición con la que corrió, tipo de disparador, datos de entrada del trigger, estado (pending → running → succeeded|failed), fechas y error estructurado.

#### Scenario: Toda ejecución es interpretable a posteriori
- **WHEN** se consulta una ejecución histórica `GET /api/v1/executions/{id}`
- **THEN** se obtienen su estado, fechas, entrada del trigger y la referencia a la versión de la definición con la que corrió

### Requirement: IO completo por bloque con duración y error
El sistema SHALL persistir en `execution_steps` la entrada resuelta, la salida, el error (si lo hubo) y la duración en ms de cada bloque, en secuencia (`execution_id, seq`), en la misma transacción que el avance de estado de la ejecución.

#### Scenario: Trace consultable por bloque
- **WHEN** se solicita `GET /api/v1/executions/{id}/trace`
- **THEN** se obtienen los pasos en orden con IO completo, duración por bloque y el punto exacto del fallo si existe

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
