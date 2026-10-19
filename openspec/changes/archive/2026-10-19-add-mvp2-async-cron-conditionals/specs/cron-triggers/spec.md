# Spec Delta: cron-triggers

## Purpose

Programación temporal de puentes: un puente puede dispararse por schedule (cron estándar con timezone) sin llamador externo, con política explícita para solapamientos y para los disparos perdidos durante una caída, y con disparo manual inmediato para probarlo.

## ADDED Requirements

### Requirement: Trigger cron en el puente
El sistema SHALL aceptar `{type: "cron", schedule, timezone}` como trigger del puente: la expresión cron SHALL ser de 5 campos estándar y la timezone SHALL ser una zona IANA válida; ambas se validan al publicar y quedan congeladas en el snapshot de la versión. Un puente con trigger cron no SHALL estar expuesto en el plano de invocación `/x`.

#### Scenario: Publicar puente con cron válido
- **WHEN** se publica un puente con `{type: "cron", schedule: "*/5 * * * *", timezone: "Europe/Madrid"}`
- **THEN** la versión inmutable congela schedule y timezone y el scheduler comienza a programarlo

#### Scenario: Schedule inválido bloquea la publicación
- **WHEN** la definición declara `schedule: "no-es-cron"` o una timezone desconocida
- **THEN** la publicación falla con 422 Problem Details indicando el campo inválido

### Requirement: Encolado puntual según schedule
El scheduler SHALL calcular la próxima ocurrencia de cada puente cron activo y encolar su ejecución con `run_at` igual al instante programado, `trigger_type: "cron"` y `trigger_data` con el instante programado; el ejecutor la reclama igual que cualquier otra fila de la cola.

#### Scenario: La ejecución nace programada, no inmediata
- **WHEN** el scheduler programa la ocurrencia de las 09:00 de un puente cada 5 minutos
- **THEN** la fila queda `pending` con `run_at` = 09:00 y `trigger_data.scheduled_at` = 09:00

### Requirement: Política de solapamiento
Si al llegar la siguiente ocurrencia la ejecución anterior del mismo puente sigue `pending` o `running`, el sistema SHALL saltarse la ocurrencia y SHALL registrarla en `missed_triggers` con `reason: "overlap"` (política fija skip en MVP2, sin encolar una segunda ejecución concurrente).

#### Scenario: Un puente lento no acumula ejecuciones
- **WHEN** la ejecución de las 09:00 sigue `running` a las 09:05
- **THEN** la ocurrencia de las 09:05 no se encola y queda registrada como `missed_triggers(reason: "overlap")`

### Requirement: Recuperación de disparos perdidos
Tras una caída, al arrancar el scheduler SHALL detectar las ocurrencias vencidas durante el tiempo muerto: SHALL encolar únicamente la más reciente y SHALL registrar el resto en `missed_triggers` con `reason: "downtime"`.

#### Scenario: Downtime de una hora en un puente cada 5 minutos
- **WHEN** el scheduler arranca tras 60 minutos caído para un puente con frecuencia de 5 minutos
- **THEN** encola solo la ocurrencia más reciente vencida y registra las ~11 restantes como `downtime`

### Requirement: Disparo manual inmediato
El sistema SHALL exponer `POST /api/v1/bridges/{id}/run` en el plano de administración para encolar una ejecución inmediata (`trigger_type: "manual"`) de la versión publicada activa del puente, sin esperar su schedule ni su trigger.

#### Scenario: Probar un cron a demanda
- **WHEN** se invoca `POST /api/v1/bridges/{id}/run` de un puente cron publicado
- **THEN** se encola una ejecución `manual` con `run_at` inmediato y se devuelve su `executionId`
