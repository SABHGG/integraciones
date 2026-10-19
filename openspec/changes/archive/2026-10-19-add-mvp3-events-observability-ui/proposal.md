# Proposal: add-mvp3-events-observability-ui

## Why

Con MVP2 el disparo queda cubierto (HTTP sync/async, cron, webhook), pero la plataforma aún no compone puentes entre sí ni se puede depurar sin ensayo real: no hay eventos, ni dry-run, ni replay, los secretos no rotan, el IO persistido muestra todo lo que produjo el puente y toda la administración exige curl. El MVP3 cierra el roadmap: composición por eventos, herramientas de prueba/depuración, bóveda con rotación, enmascaramiento y la primera UI propia.

## What Changes

- **Eventos/colas**: puentes suscritos a topics; publicación de eventos por API y emisión opcional al completar una ejecución (éxito/fallo); fan-out asíncrono vía la cola existente.
- **Dry-run con mocks**: ejecución de prueba del borrador con datos de prueba y salidas mock por instancia, sin crear ejecuciones persistentes.
- **Replay**: re-ejecución de una ejecución previa con su misma versión congelada y su misma entrada, enlazada a la original.
- **Bóveda con rotación**: los secretos pasan a versionarse (cada actualización crea versión inmutable) y las ejecuciones registran qué versión de credencial usaron.
- **Enmascaramiento**: reglas declarativas por puente que enmascaran campos del IO persistido (trace, detalle y webhook entregan el dato enmascarado).
- **UI propia (consola web)**: dashboard de ejecuciones con trace, gestión visual de puentes y bloques, formularios generados desde los metadatos `ui` y editor de código con errores de publicación en línea; servida por el mismo binario.

## Capabilities

### New Capabilities
- `event-triggers`: suscripción de puentes a topics, publicación de eventos y emisión al completar ejecuciones.
- `dry-run`: ensayo del borrador con mocks y datos de prueba, sin persistir ejecuciones.
- `execution-replay`: re-ejecución enlazada de una ejecución previa con su versión e entrada originales.
- `io-masking`: reglas declarativas de enmascaramiento aplicadas al IO observable.
- `web-console`: consola web servida por el binario (dashboard, puentes, formularios `ui`, editor de bloques, trazas).

### Modified Capabilities
- `credentials-vault`: los secretos se versionan con rotación auditable; el runtime registra la versión usada por ejecución.
- `execution-store`: el IO observable respeta las reglas de enmascaramiento; `trigger_data` registra enlaces de replay y de evento origen.

## Impact

- **Affected specs**: `event-triggers`, `dry-run`, `execution-replay`, `io-masking`, `web-console` (ADDED); `credentials-vault`, `execution-store` (MODIFIED).
- **Affected code**: `crates/domain` (gramática de masks), `crates/engine` (modo dry-run, registro de credenciales usadas), `crates/api` (eventos, replay, masks, assets de la consola), migraciones nuevas (versiones de credencial), primer frontend (estático embebido).
- **Compatible hacia atrás**: sin reglas de mask ni topics declarados, el comportamiento queda idéntico a MVP2; la bóveda migra a versiones conservando el secreto vigente como versión actual.
- **Orden de aplicación**: este change asume MVP2 aplicado y archivado — los deltas MODIFIED de `execution-store` parten del estado post-MVP2 (pasos `skipped`).
- **Fuera de alcance**: identidad real con login de usuarios (la consola usa API keys, el schema ya está preparado), Redis como cola, marketplace de bloques.
