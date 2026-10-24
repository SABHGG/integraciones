# Talleres prácticos: todos los tipos de integración

Serie de talleres guiados para aprender a construir integraciones reales con
bloques y puentes. Cada taller resuelve **un tipo de integración** de punta a
punta — bloques, puente, publicación, disparo y observación — con una API
pública real, sin claves de pago y operable en tu `dev.sh` local.

Cada paso muestra las **dos rutas**: la consola (con los botones y
formularios exactos) y la API por `curl`. Los detalles finos de cada pieza
viven en las guías de referencia; los talleres te llevan de la mano.

## La serie

| # | Taller | Tipo de integración | Lo que aprendes |
|---|---|---|---|
| 1 | [Rick and Morty](taller-rick-and-morty.md) | **HTTP bajo demanda** | Bloques `http` + `custom_logic`, refs `$.`, dry-run, publicar, invocar, trace, evolución v2 |
| 2 | [Recopilación programada](taller-cron-programado.md) | **Programada (cron)** | Trigger cron (5 campos + timezone), recuperación tras caídas, condiciones `when`, entrega por webhook |
| 3 | [API autenticada](taller-credenciales-api.md) | **Con credenciales** | Bóveda write-only, `{$credential:x}` en `http`, `ctx.credential` en `custom_logic`, `mask`, rotación |
| 4 | [Eventos y webhooks](taller-eventos-webhooks.md) | **Dirigida por eventos** | `emit`, trigger `event`, fan-out a varios puentes, webhook firmado (HMAC) y su verificación en el receptor |

## Ruta recomendada

1. **Taller 1 primero**: sienta el vocabulario (bloque, instancia, versión,
   borrador, refs `$.`). Todo lo demás se apoya en él.
2. **Taller 2 o 3 después**, según te interese lo programado o los secretos —
   son independientes entre sí.
3. **Taller 4 al final**: encadena lo aprendido (reutiliza el puente del
   taller 1 como emisor) y introduce acoplamiento por eventos.

## Matriz de conceptos

| Concepto | T1 | T2 | T3 | T4 |
|---|:-:|:-:|:-:|:-:|
| Bloque built-in `http` | ● | ● | ● | ● |
| Bloque `custom_logic` (código, schemas, `ui`) | ● | ● | ● | ● |
| Referencias `$.` entre instancias | ● | ● | ● | ● |
| Trigger `http` (`/x/{slug}`) | ● | | ○ | ○ |
| Trigger `cron` (scheduler, timezone) | | ● | | |
| Trigger `event` (fan-out, topics) | | | | ● |
| `emit` al completar | | | | ● |
| Credenciales (`{$credential:x}`, `ctx.credential`) | | | ● | |
| `mask` (enmascarado irreversible) | | | ● | |
| `when` (pasos `skipped`) | ○ | ● | | ● |
| Webhook de resultado firmado | | ● | | ● |
| Verificación HMAC en el receptor | | | | ● |
| Dry-run, validar, publicar versiones | ● | ● | ● | ● |
| Rollback y evolución sin romper | ● | ○ | ● | |

● se practica a fondo · ○ se menciona o usa de pasada

## Requisitos comunes

- `./scripts/dev.sh` corriendo: API en `http://127.0.0.1:8080`, consola en
  `/console`, documentación en `/docs` (Scalar).
- El seed deja listo el entorno: workspace `demo`, API key `demo-key-1`,
  bloques `http-get` / `http-post` / `enriquece-cliente`, credencial
  `pago-api` y el puente demo `registro-cliente` publicado.
- Para entrar a la consola: con la API key (actúa como `owner`) o con el
  usuario demo del seed (`demo@integraciones.dev`).
- Rol mínimo por taller: `editor` para construir y publicar; `runner` basta
  para disparar, ensayar y publicar eventos.
- Para los ejemplos por API: `K='Authorization: Bearer demo-key-1'`.

## Convenciones de la serie

- **Consola primero**: cada paso indica dónde hacer clic (con las etiquetas
  reales de la UI) y trae el `curl` equivalente para automatizar.
- **Verificado contra el código**: etiquetas de la consola, sintaxis de
  `config`, firmas y payloads coinciden con lo que implementan `crates/` y
  `console/`.
- **APIs externas reales y sin clave**: rickandmortyapi.com (T1),
  api.open-meteo.com (T2), httpbin.org (T2–T4) — probadas en vivo al
  escribir los talleres.
- **Nada de secretos en el trace**: donde aparece un secreto, el taller te
  lleva a la bóveda y a `mask`.

## Guías de referencia

Los talleres no sustituyen a las guías; las usan:

- [`guia-bloques.md`](guia-bloques.md) — contrato del código, sandbox `ctx.*`, `ui`
- [`guia-puentes.md`](guia-puentes.md) — disparadores, refs `$.`, `when`/`switch`, emit/webhook/mask
- [`guia-consola.md`](guia-consola.md) — páginas, roles, rastro visual, trace
- [`errores-comunes.md`](errores-comunes.md) — diagnóstico rápido de HTTP y códigos de ejecución
