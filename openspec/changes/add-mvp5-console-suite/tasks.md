# Tasks: add-mvp5-console-suite

## 1. Fase 1 — Backend de lectura

- [ ] 1.1 `GET /api/v1/stats{?from,to}`: agregación única (serie diaria, estados, media de exitosas, desglose por puente con slug; límite de 90 días); tests de agregación con ejecuciones reales y de rango inválido 400
- [ ] 1.2 `GET /bridges/{id}/versions/{version}`: definición congelada (400 no-entero, 404 inexistente, definition idéntica al snapshot); tests
- [ ] 1.3 OpenAPI de ambos endpoints;Scalar servido al día

## 2. Fase 2 — Credenciales en la consola

- [ ] 2.1 Página de credenciales: listado con versión vigente, crear, rotar y eliminar con confirmación; errores del servidor en línea; test Vitest del flujo de formularios
- [ ] 2.2 Historial de versiones por credencial (ids y fechas, sin valores) con enlace a `currentVersionId`; test

## 3. Fase 3 — Biblioteca de bloques en la consola

- [ ] 3.1 Listado con filtros (tipo, categoría) y detalle de versiones (schemas, `ui`); tests
- [ ] 3.2 Crear/editar `custom_logic`: editor de código con errores de transpile en línea y publicación bloqueada con 422 visible; test de error de sintaxis mostrado
- [ ] 3.3 Detalle de bloques built-in (http/switch) con sus schemas y metadatos `ui`; test

## 4. Fase 4 — Versiones: diff y rollback

- [ ] 4.1 Módulo de diff estructural en cliente (rutas planas, lado a lado) con tests de Vitest
- [ ] 4.2 Vista de versiones del puente: lista paginada, selector para diff versión↔versión y borrador↔versión, adopción del contenido publicado al borrador; test con ejecución real
- [ ] 4.3 Rollback orquestado (definición → PATCH del borrador → publicar) etiquetado como «rollback de la N» con confirmación; test de que las versiones previas y ejecuciones quedan intactas

## 5. Fase 5 — Dashboard: métricas y auto-refresh

- [ ] 5.1 Sección de métricas en el dashboard: total/estados, serie diaria con huecos rellenados, duración media, filtro de rango; tests de formateo/serie
- [ ] 5.2 Desglose por puente navegable al listado filtrado por ese puente; test
- [ ] 5.3 Auto-refresh configurado (5s, apagado por defecto, pausa en pestaña oculta, sin peticiones solapadas); test del temporizador

## 6. Fase 6 — Export, import y duplicar

- [ ] 6.1 Export de borrador o de una versión congelada como JSON descargable (sin ids de workspace); test del artefacto
- [ ] 6.2 Import con creación de puente → PATCH de borrador → validate con errores en línea → publicación explícita; test con JSON inválido
- [ ] 6.3 Duplicar puente con slug/nombre sufijados y publicación opcional; test

## 7. Fase 7 — Cierre

- [ ] 7.1 README: secciones de consola (credenciales, bloques, versiones/diff/rollback, métricas, export/import) con capturas de flujo descritas
- [ ] 7.2 CI verde (build de consola + vitest + suites Rust) y push; archive con sync de specs (workspace-stats, bridge-version-access, web-console)
