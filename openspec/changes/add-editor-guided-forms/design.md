# Design

## Context

El servidor ya define y valida todo el contrato que la consola hoy pide teclear como JSON: `validate_trigger` (crates/api/src/bridges.rs) exige por tipo http/cron/event/manual los campos method+path / schedule+timezone / topic / ninguno; el contrato `ui` (crates/api/src/ui.rs) es cerrado (`title|order|widget|help`) con ids kebab-case y `ref-template` reservado; las versiones de bloque exponen `configSchema`, `inputSchema`, `output_schema` y `ui` en `GET /blocks/{id}/versions` y la consola ya las carga en el editor. La resolución de refs (crates/core/src/refs.rs) acepta mapeo directo (`$.a.b`) en input y interpolación `{$.a.b}` en strings de config, con raíces `$.trigger` y `$.now` y el resto salidas de bloques previos.

## Goals / Non-Goals

**Goals:**
- Que un puente completo (trigger, inputSchema, cadena con config/input) se construya sin escribir JSON salvo que el autor quiera.
- Que el autocompletado de `$.` derive las rutas de datos que ya están disponibles en el editor (inputSchema, output_schema de versiones previas) — una sola fuente de verdad: el contrato del servidor.
- Que los metadatos `ui` se puedan declarar desde la consola para que los formularios generados materialicen de verdad.

**Non-Goals:**
- No se cambia backend: ni `validate_trigger`, ni el contrato `ui`, ni endpoints ni migraciones.
- No hay editor visual de cadena nueva (el lineal actual se queda); ni drag & drop.
- No se duplica la matriz de validación del servidor: la validación en cliente es solo "ayuda temprana", el 422 del servidor sigue siendo la última palabra y se pinta igual que hoy.

## Decisions

**D1. Opciones de `select` desde el `configSchema` (`enum`), no desde `ui`.** El contrato `ui` es cerrado a propósito y no tiene sitio para opciones; extenderlo (`widget.select` + `options{}`) implicaría backend y una migración semántica de versiones publicadas. JSON Schema ya tiene un mecanismo estándar: `properties.<campo>.enum`. Decisión: `ui.widget.<campo> = "select"` declara la intención y el `enum` del `configSchema` aporta las opciones; sin `enum`, el campo cae a `text`. *Alternativa descartada:* ampliar `ui` con una clave `options` (rompe el cierre del contrato y exige publish server-side).

**D2. Contexto de refs en un módulo puro compartido** (`console/src/lib/refs-contexto.ts`): entrada — tipo de trigger, `inputSchema`, lista de instancias previas (`name` + `output_schema` de su versión); salida — árbol de sugerencias `$.`. Expansión JSON Schema *best-effort*: `object.properties` recursivo, `array.items` un nivel con índice de ejemplo (`0`), profundidad acotada (p. ej. 6) y siempre las raíces base (`$.now`, `$.trigger`). Los schemas con `allOf`/`if-then` no se expanden: se ofrecen sus raíces. *Alternativa descartada:* endpoint server-side que "aplane" refs (nuevo acople API↔consola para algo que el cliente ya tiene en memoria).

**D3. Autocompletado anclado al cursor del control existente**, no un nuevo editor de expresiones. El dropdown se abre con botón `$` + atajo (Ctrl/Cmd+Espacio) sobre: campos `ref-template`, textarea JSON de `config` (bloques sin `ui`) y textarea de `input`; inserta en `selectionStart` — `{$.ruta}` en config, `$.ruta` en input. *Alternativa descartada:* componente tipo CodeMirror con hint nativo en todos los campos (más dependencia y peso para el mismo valor).

**D4. Formulario de trigger como componente nuevo con estado compartido y pestaña JSON avanzada.** `FormularioTrigger` escribe/lee `draft.trigger` igual que hoy hace `FormularioInstancia`; la pestaña avanzada es la misma textarea JSON sobre el mismo estado, así la ida y vuelta es gratuita. Validaciones en cliente solo las baratas y sin dependencias: verbo en la lista de 5, path con `/`, topic con la regex del servidor, cron de 5 campos con rangos (versión ligera de `domain::cron`), y timezone contra `Intl.supportedValuesOf('timeZone')` (lista IANA nativa del runtime, cero catálogo embebido; fallback a lista mínima si el navegador no la soporta). *Alternativa descartada:* enviar siempre al servidor para validar (más viajes, peor UX en el caso frecuente).

**D5. Constructor de inputSchema con modelo intermedio y merge conservador** (`lib/schema-builder.ts`): parsea el documento vigente, separa campos simples editables (`properties.<nombre>.{type, enum?}` + `required[]`) del resto (`pattern`, `additionalProperties`, anidaciones raras…), deja el resto intocado y lo vuelve a fusionar al serializar. Los campos no simples se listan como "avanzados (se conservan)" en vez de ocultarse. *Alternativa descartada:* reconstruir el schema desde cero (destruiría restricciones finas sin aviso).

**D6. Autoría de `ui` en el formulario de publicación de versión** (bloques http y custom_logic): subsección con title, order (lista ordenable de nombres de campo), widget por campo (desplegable con los 7 ids del contrato) y help por campo. El documento generado pasa por el mismo `ui::validate` del servidor al publicar — la consola no lo duplica. Para enseñar el efecto, el formulario usa él mismo esos widgets (dogfooding): el campo `title` es un `text`, los widgets se eligen con un `select`.

## Risks / Trade-offs

- [Drift entre validación ligera en cliente y `validate_trigger`/`domain::cron`] → el servidor sigue siendo la única autoridad; los issues 422 se pintan donde ya se pintan. Test de ida y vuelta garantiza que el formulario nunca *bloquea* algo que el servidor aceptaría.
- [Schemas que el walker no expande → sugerencias incompletas] → siempre se ofrecen raíces base + edición JSON avanzada disponible; el autocompletado es ayuda, no sustituto del contrato.
- [`Intl.supportedValuesOf` ausente (navegadores viejos)] → fallback a una lista corta de zonas comunes; la validación real sigue en servidor.
- [Metadatos `ui` inválidos escritos por el autor] → el servidor ya rechaza al publicar con 422 de campo `ui`; se muestra con el mecanismo existente.
- [Textareas JSON con cursor + dropdown: complejidad de foco] → aislada en un componente (`InputConRefs`) con tests de inserción en cursor.

## Migration Plan

Sin migraciones ni despliegue especial: todo es front-end de la consola servida por el binario. Rollback = revertir el commit de consola; ningún dato persistido cambia de forma.

## Open Questions

(ninguna que bloquee implementación: D1–D6 resuelven las decisiones con impacto en specs y tareas.)
