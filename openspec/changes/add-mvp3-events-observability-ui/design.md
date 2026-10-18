# Design: add-mvp3-events-observability-ui

## Context

MVP2 habrá dejado: cola madura (`executions` + SKIP LOCKED), webhook de resultado con worker de entregas, scheduler cron en el binario y pasos `skipped` en `execution_steps`. Este change monta sobre esa base sin nueva infraestructura de estado: Postgres sigue siendo la única dependencia y el binario único el único despliegue. La UI es el primer código frontend del repo.

## Goals / Non-Goals

**Goals:**
- Composición de puentes por eventos sin acoplamiento ni orden garantizado entre suscriptores.
- Depuración sin efectos: dry-run del borrador con mocks y replay de ejecuciones reales.
- Bóveda auditable (versiones + qué versión usó cada ejecución) y IO seguro por defecto (mask).
- Consola web útil sin backend nuevo: solo assets estáticos + plano admin existente.

**Non-Goals:**
- Login de usuarios real (la consola usa API key; `users`/`workspace_members` siguen preparados para el futuro).
- Redis, bus de eventos persistente, entrega garantizada u ordenada de eventos entre puentes.
- Marketplace/compartir bloques entre workspaces.
- Framework de UI con SSR; la consola es una SPA estática embebida.

## Decisions

### D1 — Eventos sin log de eventos: fan-out directo a la cola
`POST /api/v1/events/{topic}` resuelve los suscriptores activos (join a `bridge_versions` publicadas con `trigger->>'type' = 'event'`) y encola una fila por suscriptor en la misma transacción de lectura. No hay tabla de eventos: el evento es el conjunto de ejecuciones creadas; sin suscriptores, el evento desaparece (202). Alternativa descartada: log de eventos persistido con cursores por suscriptor — infraestructura de colas real que el roadmap no exige y Postgres no necesita a esta escala. La emisión al completar (D1b) reutiliza el mismo mecanismo dentro de la transacción de finalización de la ejecución: `emit.onSuccess/onFailure` declarados en la definición, congelados en snapshot, con el sobre final como payload. Sin garantía de orden ni de entrega entre puentes: cada suscriptor es una ejecución independiente con su propio fail-fast y su propio trace.

### D2 — Dry-run: ejecución en memoria del borrador, mocks por instancia
El endpoint ejecuta el borrador con el mismo motor en modo sin persistencia: sin filas en `executions`/`execution_steps`, sin webhook, sin `emit`; el trace viaja en la respuesta. `mock: {output}` por instancia corta la invocación (sandbox/HTTP) y alimenta el contexto; las credenciales sí se resuelven para bloques no mockeados (mismo camino de bóveda). El timeout del puente aplica igual. Riesgo documentado: un dry-run sin mocks hace llamadas reales — es el comportamiento previsto ("datos de prueba + bloques mock"), los mocks existen justamente para evitarlo.

### D3 — Replay: clonar ejecución, no estado
`POST /api/v1/executions/{id}/replay` inserta una fila nueva copiando `bridge_version_id` y la entrada del trigger de la original, con `trigger_data.replayOf`. Corre por la cola normal — mismo worker, mismo timeout, mismo masking, misma entrega webhook. Sin límite de replays en MVP3 (el listado muestra la cadena). No se re-ejecuta el estado intermedio: siempre desde el trigger.

### D4 — Bóveda versionada con migración conservadora
Migración: nueva tabla `credential_versions` (id, credential_id, valor cifrado, rotated_at, created_at); la migración copia el secreto vigente de `credentials` como versión 1 y deja en `credentials` una columna `current_version_id`. `ctx.credential` resuelve la versión vigente; la ejecución registra en `executions.credentials_used` (jsonb) los pares `{credential_id, version_id}` resueltos durante sus pasos, en la misma transacción del trace. El listado del historial solo expone metadatos. Write-only se mantiene en todas las versiones.

### D5 — Mask en el punto de persistencia, reglas congeladas por versión
`mask: [paths]` vive en la definición (como `webhook`/`emit`) y viaja en el snapshot; el ejecutor aplica la sustitución por `***` justo antes de persistir cada paso y el sobre final, dentro de la transacción del trace — el valor original nunca toca el store. El webhook entrega lo persistido (enmascarado). Las rutas usan la gramática `$.` existente (raíces `$.trigger`/salidas por nombre), resueltas con el mismo resolvedor del motor — sin parser nuevo. El coste es la irreversibilidad: para depurar con valores reales, dry-run.

### D6 — Consola web: SPA estática embebida, cero backend nuevo
Assets embebidos en el binario (include en compile-time, mismo patrón que `openapi.json`), servidos bajo `/console` con auth Bearer por API key introducida en el login (guardada en el almacenamiento del navegador). El framework se decide en la tarea de arranque de la fase 5 (criterio: SPA ligera sin SSR, build estático, TypeScript). Los formularios se generan desde el campo `ui` de la versión del bloque (contrato definido en MVP1: tipos, label, required, options); bloque sin `ui` → editor JSON genérico validado contra el schema. El editor de puentes no inventa validación propia: muestra los 422 del validador del servidor sobre el canvas/editor (una sola fuente de verdad).

## Risks / Trade-offs

- **Fan-out transaccional de eventos**: un topic con cientos de suscriptores alarga la transacción de publicación → límite práctico documentado; el log de eventos con cursores queda como escape futuro.
- **Eventos sin entrega garantizada**: si un suscriptor no tiene versión publicada en el instante del fan-out, pierde ese evento → documentado; cron/replay cubren la recomposición manual.
- **Dry-run con bloques reales** puede disparar efectos externos (llamadas HTTP reales) → es explícito en la spec; los mocks son el mecanismo de aislamiento.
- **Máscara irreversible**: un mask mal definido pierde datos de depuración → el dry-run (sin persistencia) es la vía de ensayo con valores reales.
- **API key en el navegador**: la consola hereda la seguridad del plano admin (una key por workspace, sin identidad por usuario) → reconocido en el roadmap; identidad real queda para el hito posterior.
