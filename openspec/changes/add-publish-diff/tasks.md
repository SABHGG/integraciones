# Tasks

## 1. Filtrado de rutas (lib/diff.ts)

- [x] 1.1 Añadir `lineasDeSecciones(lineas, secciones)` a `console/src/lib/diff.ts`: conserva rutas cuya primera parte pertenece a las secciones y descarta `blocks.*.code` (resolución del servidor); tests que cubren el filtro, la exclusión de `blocks.0.code` y el caso «sin líneas» (verificación: `npm run test` en verde).

## 2. Render compartido

- [x] 2.1 Extraer el render de líneas de `PanelVersiones` a `console/src/ui/tabla-diff.tsx` (props `lineas`, `etiquetaIzq`, `etiquetaDer`, `plegable`; reglas de El rastro: mono para rutas/valores, `AUSENTE` atenuado, sin croma de estado) y adoptarlo en `paginas/versiones.tsx` (verificación: `npm run build` sin errores y el diff de versiones se ve igual que antes en `npm run dev`).

## 3. Confirmación con diff

- [x] 3.1 En `paginas/bridges.tsx`: estado `confirmacion` enriquecido (`{resumen, primeraVersion?, sinDiff?, lineas?}`); `pedirPublicacion` obtiene `GET /bridges/{id}/versions/{latestVersion} → {definition}` cuando hay versión previa, calcula `diffEstructural(draft, definition)` + `lineasDeSecciones`, y gestiona primera versión (sin GET) y fallo de carga (nota sin bloquear) (verificación: `npm run test` para las funciones y `npm run build`).
- [ ] 3.2 Panel de confirmación: resumen orgánico + «N diferencias» (`resumenDiff`) + `<details>` con `TablaDiff` acotado, o «primera versión — no hay una anterior con la que comparar», o nota «no se pudo comparar con v{n}»; «Publicar ahora» queda en variante `peligro` (verificación: recorrido manual de los 5 escenarios del delta con `npm run dev`).
- [ ] 3.3 Caso «idénticos»: borrador igual a la última versión muestra «idénticos» y permite publicar (verificación: manual en `npm run dev`).

## 4. Cierre

- [ ] 4.1 Suite completa y build de producción: `npm run test` y `npm run build` en verde; `impeccable detect --json console/src/paginas/bridges.tsx console/src/ui/` sin hallazgos; recorrido de los 5 escenarios del delta (verificación: comandos en verde + lista recorrida).
