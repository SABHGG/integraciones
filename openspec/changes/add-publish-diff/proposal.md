# Proposal

## Why

La confirmación de publicación muestra hoy un resumen orgánico en mono (`http POST /x/demo · 3 bloques`): honesto, pero ciego a lo que de verdad cambia. El posicionamiento del producto es «integraciones como código» (versiones inmutables con diff y rollback), y el momento firma de ese posicionamiento es justo antes de congelar: ver el **diff estructural contra la última versión publicada**. La maquinaria ya existe y está probada — `diffEstructural`/`resumenDiff` (lib/diff.ts, MVP5) y el endpoint que entrega la definición congelada — solo falta conectarla a la confirmación del editor.

## What Changes

- La confirmación de publicación muestra el **diff estructural** entre el borrador actual y la última versión publicada: resumen (`resumenDiff`) y lista plegable de líneas (`ruta: izq → der`).
- **Primera publicación** (sin versión previa): la confirmación lo dice explícitamente y no muestra diff (no hay contra qué comparar).
- El render del diff se **extrae a un componente compartido** en `ui/`; `PanelVersiones` (diff/rollback de versiones) adopta el mismo componente — cero duplicación.
- Las rutas del diff se **filtran a las secciones editables del borrador** (`trigger`, `inputSchema`, `blocks`, `mask`, `emit`, `webhook`): el normalizado del servidor no genera ruido en la confirmación.
- Sin cambios de backend: `GET /bridges/{id}` entrega `latestVersion` y `GET /bridges/{id}/versions/{n}` entrega `{ definition }` — los mismos que ya usa `PanelVersiones`.

## Capabilities

### New Capabilities

(ninguna)

### Modified Capabilities

- `web-console`: la confirmación de publicación de versión muestra el diff estructural contra la última versión publicada antes de congelar (hoy muestra solo un resumen orgánico).

## Impact

- **Consola** (`console/src/`): `paginas/bridges.tsx` (pedir la definición anterior en `pedirPublicacion` y renderizar el diff en el panel de confirmación), nuevo `ui/tabla-diff.tsx` (render extraído), `paginas/versiones.tsx` (adopta el componente compartido). `lib/diff.ts` queda intacto.
- **Datos**: dos GET ya existentes por confirmación (uno solo cuando hay versión previa); sin migraciones ni cambios de contrato.
- **Tests**: vitest para el filtrado de secciones, el estado «primera publicación» y el uso de `resumenDiff`; detector Impeccable al final.
