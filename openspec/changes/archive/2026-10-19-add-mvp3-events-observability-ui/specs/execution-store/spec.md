# Spec Delta: execution-store

## MODIFIED Requirements

### Requirement: IO completo por bloque con duración y error
El sistema SHALL persistir en `execution_steps` la entrada resuelta, la salida, el error (si lo hubo) y la duración en ms de cada bloque, en secuencia (`execution_id, seq`), en la misma transacción que el avance de estado de la ejecución. El estado del paso SHALL admitir `succeeded`, `failed` y `skipped`: los pasos no ejecutados por enrutado condicional (`when` falso o ruta de `switch` no tomada) SHALL registrarse como `skipped`, sin input ejecutado, salida ni duración. Los campos cubiertos por reglas de enmascaramiento de la versión SHALL persistirse ya enmascarados (`***`), de forma irreversible.

#### Scenario: Trace consultable por bloque
- **WHEN** se solicita `GET /api/v1/executions/{id}/trace`
- **THEN** se obtienen los pasos en orden con IO completo, duración por bloque y el punto exacto del fallo si existe

#### Scenario: Los pasos condicionales no tomados quedan en el trace
- **WHEN** un `switch` enruta por la ruta `vip` dejando la ruta `standard` sin ejecutar
- **THEN** el trace muestra el paso de `standard` como `skipped`, en su `seq` original y sin IO

#### Scenario: El campo enmascarado se persiste enmascarado
- **WHEN** la versión del puente declara `mask` sobre un campo que un bloque produce
- **THEN** `execution_steps` guarda `***` en ese campo y el valor original no existe en ninguna tabla

### Requirement: Registro completo de ejecuciones
El sistema SHALL persistir cada ejecución con: referencia al puente y a la versión exacta de la definición con la que corrió, tipo de disparador, datos de entrada del trigger, estado (pending → running → succeeded|failed), fechas y error estructurado. `trigger_data` SHALL registrar además el origen del disparo: topic y payload del evento, ejecución original en un replay, o instante programado en un cron.

#### Scenario: Toda ejecución es interpretable a posteriori
- **WHEN** se consulta una ejecución histórica `GET /api/v1/executions/{id}`
- **THEN** se obtienen su estado, fechas, entrada del trigger y la referencia a la versión de la definición con la que corrió

#### Scenario: El origen del disparo es rastreable
- **WHEN** se consulta una ejecución disparada por evento o creada por replay
- **THEN** `trigger_data` identifica el topic y payload del evento, o la ejecución original del replay, respectivamente
