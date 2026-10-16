# Tasks: add-mvp1-core

> El orden de fases es el orden de construcción. La Fase 2 (sandbox) es la ruta crítica técnica: valida la apuesta de mayor riesgo antes de construir encima. Tras la tarea 1.1, las fases 0 y 1 pueden paralelizarse.

## 1. Fase 0 — Cimientos

- [ ] 1.1 Crear estructura Cargo (single crate o workspace), CI básico, `docker-compose.yml` con Postgres, `.gitignore`
- [ ] 1.2 Migraciones iniciales del esquema completo (todas las tablas e índices de design.md; los recursos MVP2/MVP3 quedan creados pero vacíos)
- [ ] 1.3 Servidor HTTP base: enrutado de dos planos, auth `Authorization: Bearer` con API key global (hash en `api_keys`), manejador de errores RFC 9457 (application/problem+json), tracing estructurado

## 2. Fase 1 — Sandbox (ruta crítica)

- [ ] 2.1 Wrapper rquickjs: crear isolate fresco, aplicar `set_memory_limit`, interrupt handler (timeout por puente en ms), destruir tras ejecución
- [ ] 2.2 Cargar bloque JS compilado e invocar `export default(input, ctx)`; capturar salida síncrona, Promise resuelta y `throw` (convertido a error estructurado del ejecutor)
- [ ] 2.3 Host functions de `ctx`: `ctx.log` (estructurado), `ctx.crypto` (hmac/sha/base64), helpers JSON
- [ ] 2.4 `ctx.http.request({url, method, headers, body})` sobre reqwest: timeout obligatorio propio (además del interrupt del isolate), allowlist opcional de dominios, respuesta como texto o JSON parseado
- [ ] 2.5 Suite de pruebas con bloques trampa: bucle infinito (verifica interrupt), throw, agotamiento de memoria, fetch lento, Promise colgante — todos deben terminar en error del ejecutor sin colgar el worker

## 3. Fase 2 — Ejecutor

- [ ] 3.1 Resolución de referencias `$.`: parser de rutas de puntos (segmentos alfanuméricos y numéricos), raíces `$.trigger` y `$.now`, refs a nodo completo; ref no resuelta = fail-fast con mensaje quirúrgico
- [ ] 3.2 Máquina de ejecución de cadena lineal: iterar bloques instanciados desde `bridge_versions.definition`, resolver inputs (mapeo + interpolación de config), validar contra inputSchema del bloque, invocar sandbox, registrar salida nombrada en el contexto, fail-fast en primer error
- [ ] 3.3 Persistencia atómica de `executions` + `execution_steps` (input/output/error/duration_ms por bloque) en la misma transacción; máquina de estados pending → running → succeeded|failed
- [ ] 3.4 Validador JSON Schema en Rust para el subconjunto estricto (types básicos, properties, required, enum, items, format) — usado por validación de entrada, publicación y dry-run futuro

## 4. Fase 3 — API

- [ ] 4.1 API de blocks: CRUD + `POST /blocks/{id}/versions` (publicar: transpilar TS con swc, validar sintaxis, schemas+ui contra el subconjunto, guardar JS) + listado de versiones; DELETE bloqueado (409) si un puente lo referencia
- [ ] 4.2 API de bridges: CRUD de borrador + `POST /bridges/{id}/validate` (estructura: cadena lineal, entrada única, sin huérfanos; compatibilidad de schemas campo a campo; validación de refs) + `POST /bridges/{id}/versions` (publicar versión inmutable con snapshot JSONB)
- [ ] 4.3 API de executions: `GET /executions` (filtros bridge/status/fechas + paginación por cursor), `GET /executions/{id}`, `GET /executions/{id}/trace`
- [ ] 4.4 Plano de invocación `/x/{slug}`: matching de method del trigger del puente, errores de transporte (404/405/413/415 con Problem Details), ejecución sync y respuesta 200 con sobre `{success, data, error?, executionId}`, códigos de error estables
- [ ] 4.5 Validación de entrada: si el puente declara inputSchema, validar `$.trigger.body` antes de ejecutar; inválido → 200 con `VALIDATION_FAILED` sin gastar isolate
- [ ] 4.6 Bloques built-in de la biblioteca base: `http-get`/`http-post` con config (url con interpolación `{$.ref}`, headers, body) y resolución de `ctx.credential(name)`
- [ ] 4.7 Bóveda mínima: `POST/PATCH/DELETE /credentials` (secreto write-only cifrado, nunca devuelto) + host function `ctx.credential(name)` que resuelve en el host

## 5. Fase 4 — Cierre

- [ ] 5.1 Bloque CUSTOM_LOGIC de ejemplo (biblioteca) + puente demo de 3 bloques encadenados (http-get → custom → http-post)
- [ ] 5.2 Prueba end-to-end: invocar `/x/{slug}` → 200 con sobre → verificar en el store el trace completo con IO por bloque
- [ ] 5.3 Job de retención: purga de `execution_steps` a 30 días y `executions` a 1 año (configurables)
- [ ] 5.4 README mínimo: cómo crear biblioteca, publicar puente e invocarlo (guía del primer usuario)
