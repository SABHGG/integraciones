# execution-replay Specification

## Purpose
Re-ejecución controlada de una ejecución previa: repetir una ejecución con su misma versión congelada y su misma entrada para reproducir un resultado — o verificar una corrección — sin rehacer el disparo original ni editar el puente.

## Requirements

### Requirement: Replay con versión y entrada originales
El sistema SHALL exponer `POST /api/v1/executions/{id}/replay` que crea una nueva ejecución usando la MISMA versión congelada de la definición con la que corrió la original y la misma entrada de trigger; la nueva ejecución SHALL correr por la cola normal (`trigger_data.replayOf` = id original) y SHALL estar sujeta a las mismas reglas de timeout, idempotencia de entrega y enmascaramiento.

#### Scenario: Replay reproduce la ejecución
- **WHEN** se lanza replay de una ejecución http que corrió con la versión 3 del puente
- **THEN** se encola una nueva ejecución contra la versión 3 (no el borrador actual) con el mismo body del disparo original

#### Scenario: El replay no muta el original
- **WHEN** un replay termina failed
- **THEN** la ejecución original conserva su estado, su trace y su enlace intactos

### Requirement: Enlace visible entre original y replays
El sistema SHALL exponer el enlace en ambos sentidos: el detalle de una ejecución replay identifica la ejecución original y el detalle de la original lista sus replays con su estado.

#### Scenario: Rastrear de dónde viene un replay
- **WHEN** se consulta el detalle de una ejecución creada por replay
- **THEN** la respuesta incluye `trigger_data.replayOf` y el detalle de esa ejecución original es resoluble

#### Scenario: Historial de replays de una ejecución
- **WHEN** se consulta el detalle de una ejecución replayeada tres veces
- **THEN** la respuesta lista las tres ejecuciones derivadas con su estado y fecha
