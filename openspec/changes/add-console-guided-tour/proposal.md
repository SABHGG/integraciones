# Proposal

## Why

La consola cubre ya todo el producto en cinco páginas, pero nada explica en pantalla qué hace cada una: un usuario nuevo recién logueado se enfrenta a Ejecuciones, Puentes, Miembros, Credenciales y Bloques sin orientación, y la única ayuda posible hoy es la guía externa `docs/guia-consola.md`. Un tour interactivo dentro de la propia consola elimina esa curva de entrada sin añadir documentación que envejece ni tocar el backend.

## What Changes

- **Tour interactivo de onboarding** en `/console` (usuario ya logueado) construido con **driver.js** instalado como dependencia npm — nunca por CDN, porque la consola es offline-first (fuentes autoalojadas, sin salida a internet).
- **Disparo automático la primera vez**: al entrar a la consola, si el navegador no ha visto el tour, arranca solo. Queda además un botón **"Ver tutorial"** en el shell para relanzarlo bajo demanda.
- **Persistencia del "ya lo vi" en `localStorage`** del navegador: **cero backend nuevo, cero endpoints nuevos** (regla dura del proyecto: la consola habla solo con el plano admin existente).
- **Alcance adaptado al rol**: los pasos se filtran según el rol de la sesión (`viewer`/`runner`/`editor`/`owner`), en coherencia con la UI por rol y el espejo `puede()` de `console/src/lib/roles.ts`. Los pasos de Miembros (solo `owner`) o de edición (`editor`+) no aparecen para un `viewer`; el dashboard y el trace son comunes.
- **Degradación segura**: un paso cuyo selector no exista en el DOM se filtra en lugar de romper el tour; el tour se destruye limpiamente al navegar o desmontar; se respeta `prefers-reduced-motion`.

Fuera de alcance: persistencia del estado del tour en el servidor (sin tabla ni endpoint), tour multi-idioma (solo español, como el resto de la consola), hints/checklists persistentes de driver.js, tours por página independientes entre sí, y cualquier cambio en el ejecutor, la API o la base de datos.

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `web-console`: se añade el requisito de tour guiado de onboarding — lanzamiento automático solo la primera vez por navegador, botón visible para relanzarlo, pasos filtrados por rol, sin endpoints nuevos, limpieza correcta y respeto a `prefers-reduced-motion`.

## Impact

- **Consola**: `console/package.json` (nueva dependencia `driver.js`), un módulo de pasos por rol en `console/src/lib/` (lógica pura, testeable), un helper de almacenamiento en `localStorage`, el montaje del tour y el botón "Ver tutorial" en `console/src/main.tsx`, y estilos del popover mapeados a los tokens de El rastro.
- **Backend**: nada. Sin migraciones, sin endpoints, sin cambios en el plano admin.
- **Bundle**: ~20 KB gzip adicionales en el bundle de la consola — aceptable frente al criterio offline-first.
- **Compatibilidad**: aditivo; los usuarios existentes solo verán el tour si el navegador no lo registró con la clave/versión vigente.
- **Documentación**: `docs/guia-consola.md` incorpora la sección del tour.
