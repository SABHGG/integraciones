# Design

## Context

Ver `proposal.md` — Why. Estado actual que condiciona el approach:

- **Transpile**: `crates/api/src/transpile.rs` ejecuta el pipeline TS→JS (parse → exigir `export default` → resolver/strip/fixer → emit ES2022). Su comentario cita la decisión vigente: «la versión inmutable guarda el JS resultante. El runtime nunca ve TS». Este design la **extiende**: el runtime sigue sin ver TS; lo que cambia es que el fuente también se conserva.
- **Publicación**: `crates/api/src/blocks.rs` — `VERSION_COLUMNS` (`id, block_id, version, code, config_schema, input_schema, output_schema, ui, created_at`), `BlockVersionDto` con `#[serde(rename_all = "camelCase")]` y `code: Option<String>`, `publish_version` (custom_logic exige `request.code`, transpila y guarda el JS; built-in `http` inserta `code` NULL). `GET /blocks/{id}/versions` ya devuelve las versiones paginadas por cursor.
- **Migraciones**: `crates/api/migrations/0001..0007_*.sql`, aplicadas solas al arrancar (`sqlx::migrate!`). La siguiente es `0008_*`.
- **OpenAPI embebido**: `crates/api/openapi.json` (`include_str!` en `docs.rs`), servido en `/openapi.json` con Scalar en `/docs`; el schema `BlockVersion` documenta hoy `code` como «el JS transpilado».
- **Consola**: `console/src/paginas/blocks.tsx` — `DetalleBloque` ya recibe `versiones: VersionBloque[]` (`code: string | null`); la tabla pinta número y fecha; un único `<details>` muestra el input schema de la última versión; el editor de nueva versión (`EditorCodigo`) arranca de una plantilla fija en `useState`; `administra = puede(rol, "administrar-recursos")`. Tests de consola en `console/src/lib/` con `environment: "node"` (lógica pura).

## Goals / Non-Goals

**Goals:**
- Que una versión `custom_logic` publicada a partir de este change conserve el fuente original **y** el JS transpilado, sin cambiar lo que ejecuta el sandbox.
- Que cualquiera pueda entender qué hace un bloque leyendo su detalle, con etiquetas honestas para lo que no tiene fuente.
- Que publicar una versión nueva parta del código real del bloque.
- Cambio aditivo y de un solo artefacto desplegable (binario único: migración + API + consola embebida).

**Non-Goals:**
- Recuperar fuentes históricas (no existen) o deducirlos.
- Type-checking del TypeScript en publicación.
- Endpoint nuevo, campo nuevo en el cuerpo de publicación (`code` sigue siendo el TS que se envía) o cambio en el contrato del ejecutor.
- Visor con resaltado/edición por versión: el detalle es lectura con el patrón `pre` mono ya usado para schemas.
- Mostrar código de bloques built-in `http`.

## Decisions

### D1 — Columna nueva `block_versions.source_code TEXT NULL` en `0008_block_source_code.sql`
Un `ALTER TABLE block_versions ADD COLUMN source_code TEXT` nullable. `NULL` significa «esta versión no tiene fuente original», que cubre los dos casos legítimos: built-in `http` (nunca tiene código) y versión `custom_logic` publicada antes del change.

Alternativas descartadas:
- **Reutilizar `code` con un JSON `{source, js}`**: rompe a todos los consumidores de `code` y obliga al ejecutor a parsear. Inaceptable para un contrato que debe quedar intacto.
- **Tabla aparte `block_version_sources`**: un join extra en cada lectura para un dato 1:1 que siempre se lee junto a la versión, y una escritura en dos tablas dentro de la misma transacción sin ganancia.
- **Columna `NOT NULL DEFAULT ''`**: un string vacío no distingue «sin fuente» de «fuente vacía»; `NULL` es la única representación honesta de la ausencia.

### D2 — Sin backfill
No se rellena `source_code` para las versiones existentes: `source_code = code` mentiría, porque el JS emitido no es el fuente que alguien escribió. Tampoco se intenta reconstruir (un «un-transpile» no existe). Las versiones viejas quedan con `source_code` NULL para siempre y la UI lo dice.

### D3 — El API expone `sourceCode` junto a `code`, y `code` mantiene su semántica
`BlockVersionDto` gana `source_code: Option<String>` → `sourceCode` (el `rename_all = "camelCase"` ya vigente). `code` **no** cambia de significado: sigue siendo el JavaScript que ejecuta el sandbox. Ambas columnas entran en `VERSION_COLUMNS` (orden = orden de los campos del `FromRow`) y en el `INSERT` de `publish_version`. Sin endpoint nuevo: `GET /blocks/{id}/versions` y la respuesta de publicación ya devuelven el DTO completo, así que el cambio es aditivo para cualquier consumidor.

Se documenta la diferencia en el comentario del DTO, en la descripción de `openapi.json` y en `docs/guia-bloques.md`, porque «dos campos de código» es la confusión más probable de este change.

### D4 — La publicación guarda ambos artefactos
En `publish_version`, para `custom_logic`: `fuente = request.code` (el TS validado) y `js = transpile_typescript(request.code)`; el `INSERT` escribe los dos. Un error de transpile sigue devolviendo 422 sin crear versión, así que nunca hay fuente sin JS ni JS sin fuente en una versión nueva. Built-in `http` sigue insertando `code` NULL y ahora también `source_code` NULL.

### D5 — Consola: visor de código por versión con un helper puro que decide qué mostrar
La decisión «¿qué código muestro y con qué etiqueta?» vive en un helper puro (`console/src/lib/`, junto a `diff.ts`/`bloques.ts`) probado con vitest (`environment: "node"`): recibe la versión y el tipo de bloque y devuelve el texto, la etiqueta y si no hay nada que mostrar. Reglas:

- `sourceCode` presente → el fuente original, sin etiqueta (es lo que el autor escribió).
- `sourceCode` ausente y `code` presente → el JS con etiqueta «JS transpilado — versión anterior al change».
- Ambos ausentes → «built-in: sin código» (o «sin código» si el bloque no es built-in).

El render reutiliza el patrón ya existente en el detalle: un `<details>` por versión con `<pre>` mono (el mismo del input schema), en lectura para cualquier rol. Se descarta montar un `EditorCodigo` (CodeMirror) por versión: N editores por página por un contenido que solo se lee.

### D6 — Precarga del editor solo en la carga inicial
Al llegar las versiones, si `versiones[0].sourceCode` existe se carga en el editor; si no, se mantiene la plantilla actual. La precarga ocurre **una vez** (guardada con `useRef`), no en cada refetch: tras publicar, el editor conserva lo que la persona escribió en vez de saltar al texto recién publicado. Tampoco se precarga con `code`: presentar el JS emitido como fuente editable invitaría a publicarlo como si fuera el original.

### D7 — Tests
- **Rust** (`crates/api/tests/`, esquema creado por `sqlx::migrate!`): publicar un `custom_logic` guarda y devuelve `sourceCode` igual al TS enviado, con `code` distinto del fuente (es el JS emitido); el listado de versiones expone `sourceCode`; built-in `http` devuelve `code` y `sourceCode` NULL; una versión histórica (insertada sin `source_code`) se lee con `sourceCode` NULL.
- **Consola** (vitest): el helper de D5 en sus tres ramas.
- **Integrado**: `cargo test --workspace`, `npm test`, `npm run build` y verificación manual con `scripts/dev.sh` (publicar una versión, ver el fuente en el detalle, reabrir el editor precargado).

### D8 — `openapi.json` a mano, en el mismo commit que el DTO
`crates/api/openapi.json` es un documento versionado, no generado en build: se añade `sourceCode` a `BlockVersion` (tipo `["string","null"]`, requerido como `code`, porque el campo siempre está presente y puede ser null) y se ajusta la descripción del schema para explicar la diferencia entre `code` y `sourceCode`. El test de `/docs` ya valida que el documento embebido sea JSON válido.

## Risks / Trade-offs

- **La fila crece** (fuente + JS del mismo bloque, ~2×) → Aceptado: los bloques son pequeños, la versión es el registro de auditoría y Postgres ya comprime/TOAST-ea valores grandes fuera de línea.
- **Confusión `code` vs `sourceCode`** (alguien consume el que no debe) → Mitigación: nombres explícitos, `code` documentado como «el JS que ejecuta el sandbox» en DTO, OpenAPI y guía, y `sourceCode` como «lo que el autor escribió».
- **Versiones viejas sin fuente** (nadie puede readitar el original) → Mitigación: etiqueta honesta en el visor y el editor cae a plantilla en vez de fingir que el JS es el fuente. Es una limitación permanente y conocida, no un bug.
- **El fuente guardado no se re-valida al leer** → Aceptado: es inmutable y se guardó tal como se envió; lo que ejecuta el sandbox ya fue validado como JS en la publicación.
- **TypeScript laxo** (swc transpila JS válido como TS válido) → el visor muestra lo que se envió; el change no promete type-checking (non-goal) y la guía lo recuerda.
- **Un `<details>` por versión alarga el detalle** con muchos historiales → Mitigación: cerrados por defecto, mismo patrón que el schema actual; si molesta, se pagina el detalle en un change futuro.

## Migration Plan

1. `0008_block_source_code.sql` es aditiva (`ADD COLUMN` nullable, sin default, sin rewrite de tabla en Postgres 11+). Se aplica sola al arrancar el binario; no hay paso manual ni downtime.
2. Despliegue en un solo artefacto: migración + API + assets de consola van embebidos en el mismo binario, así que API y UI que entienden `sourceCode` entran juntas.
3. Rollback: el código anterior ignora la columna (los `SELECT`s no la listan), así que revertir el binario deja la base válida; la columna puede quedarse. No hace falta migración inversa.
4. Efecto en datos existentes: ninguno — todas las versiones previas quedan con `source_code` NULL y la UI las etiqueta como JS transpilado.
