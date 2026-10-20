# Tasks

## 1. Migración y persistencia del fuente

- [x] 1.1 Crear `crates/api/migrations/0008_block_source_code.sql` con `ALTER TABLE block_versions ADD COLUMN source_code TEXT` (nullable, sin default, sin backfill) — verificado: `cargo test --test blocks_api` crea el esquema con `sqlx::migrate!` y la migración aplica sin error
- [x] 1.2 Añadir `source_code` a `VERSION_COLUMNS` y a `BlockVersionDto` (`source_code: Option<String>`, serializado como `sourceCode` por el `rename_all = "camelCase"` vigente), documentando en el comentario del DTO que `code` es el JS que ejecuta el sandbox y `sourceCode` el fuente que escribió el autor — verificado: `cargo check -p api` sin errores y el campo aparece en la respuesta de los tests de 1.4
- [x] 1.3 `publish_version` guarda ambos artefactos en el `INSERT`: el TypeScript enviado en `source_code` y el JS transpilado en `code`; el bloque built-in `http` sigue publicando con `code` y `source_code` NULL y el 422 de transpile sigue sin crear versión — verificado: los tests de 1.4 pasan
- [x] 1.4 Tests en `crates/api/tests/blocks_api.rs`: publicar un `custom_logic` devuelve `sourceCode` igual al TS enviado con `code` distinto (el JS emitido); `GET /blocks/{id}/versions` expone `sourceCode`; el built-in `http` devuelve `code` y `sourceCode` null; una versión histórica insertada sin `source_code` se lee con `sourceCode` null — verificado: `cargo test --test blocks_api` verde

## 2. Contrato del API y su documentación

- [x] 2.1 Añadir `sourceCode` al schema `BlockVersion` de `crates/api/openapi.json` (tipo `["string","null"]`, requerido igual que `code`) y ajustar la descripción del schema para distinguir `code` (JS ejecutable del sandbox) de `sourceCode` (fuente original del autor) — verificado: `python3 -c "import json; json.load(open('crates/api/openapi.json'))"` sin error y `cargo test --test docs_api` verde (documento embebido válido y servido en `/openapi.json`)

## 3. Consola: visor por versión y editor precargado

- [x] 3.1 Añadir `sourceCode: string | null` al tipo `VersionBloque` de `console/src/paginas/blocks.tsx` — verificado: `npm run build` (incluye `tsc --noEmit`) compila sin errores
- [x] 3.2 Helper puro en `console/src/lib/` que decide qué código mostrar por versión y con qué etiqueta (fuente original sin etiqueta; solo JS → «JS transpilado — versión anterior al change»; sin código → «built-in: sin código»), con test de vitest que cubre las tres ramas — verificado: `npm test` verde
- [x] 3.3 Visor de código por versión en el detalle de bloque: un `<details>` por versión con el `<pre>` mono ya usado para el input schema, cerrado por defecto y en lectura para cualquier rol (`viewer` incluido) — verificado: `npm run build` y, con `scripts/dev.sh`, el detalle de un `custom_logic` publicado tras el change muestra el fuente, el de una versión anterior muestra el JS etiquetado y el de un built-in `http` dice que no tiene código
- [x] 3.4 Precargar el editor de «publicar nueva versión» con el `sourceCode` de la última versión, una sola vez (si es NULL se mantiene la plantilla actual y nunca se precarga con `code`) — verificado: con `scripts/dev.sh`, el editor de un bloque con fuente abre con ese texto, publicar crea la siguiente versión a partir de él y lo escrito no se pisa tras publicar; un bloque sin fuente sigue abriendo con la plantilla
- [x] 3.5 Actualizar `docs/guia-bloques.md`: al publicar se conserva el TypeScript original además del JS, dónde se ve en el detalle y qué significa que una versión anterior al change solo tenga JS — verificado: cada afirmación de la guía se comprueba en la consola de 3.3 y 3.4

## 4. Verificación integrada

- [x] 4.1 `cargo test --workspace` y `cargo clippy --workspace --all-targets -- -D warnings` verdes
- [x] 4.2 En `console/`: `npm test` y `npm run build` verdes
- [x] 4.3 Recorrido manual con `scripts/dev.sh`: publicar una versión `custom_logic`, ver el fuente en el detalle, confirmar en la base que `code` es JS y `source_code` el TypeScript, y comprobar que `/openapi.json` documenta `sourceCode`; `npx -y @fission-ai/openspec@latest validate "add-block-source-code" --type change` sin errores
