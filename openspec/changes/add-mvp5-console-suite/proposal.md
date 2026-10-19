# Proposal

## Why

La consola del MVP3 cubre ejecuciones y edición de puentes, pero deja fuera superficies que la API ya ofrece (credenciales con su historial de versiones, la biblioteca de bloques) y funcionalidades de operación que hoy exigen curl: métricas agregadas, comparación y rollback de versiones, y compartir puentes. Este change dedica un hito completo a cerrar esa brecha: que todo lo que el producto sabe hacer sea visible y operable desde `/console`, más las piezas de solo lectura de API que faltan (definición de una versión concreta, métricas agregadas).

`add-mvp4-users-roles` queda en espera: la consola sigue autenticándose con API key de workspace (acceso aceptado en MVP3).

## What Changes

- **Credenciales en la consola**: página con listado, creación, rotación y historial de versiones (ids y fechas, jamás valores) consumiendo la API de la bóveda versionada existente.
- **Biblioteca de bloques en la consola**: listado con filtros, detalle de versiones (schemas y metadatos `ui`), creación y edición de bloques `custom_logic` con el editor de código y publicación con los errores del validador en línea.
- **Métricas del workspace** (backend, solo lectura): `GET /api/v1/stats` agrega ejecuciones por día, tasa de éxito y duración media (global y por puente) para un rango de fechas; el dashboard las grafica y añade auto-refresh configurable (polling de los endpoints existentes).
- **Versiones con diff y rollback** (backend mínimo + consola): `GET /bridges/{id}/versions/{version}` devuelve la definición congelada de una versión; la consola lista versiones, muestra el diff visual entre dos versiones (o borrador vs versión) y hace rollback volviendo a publicar una definición anterior como versión nueva (nunca reescribe el pasado).
- **Export/import/duplicar** (solo consola): exportar un puente (draft o versión) como JSON descargable, importarlo en cualquier workspace y duplicar — orquestación sobre los endpoints existentes de creación, borrador y publicación.

## Capabilities

### New Capabilities

- `workspace-stats`: métricas agregadas de ejecuciones del workspace (solo lectura, plano admin).
- `bridge-version-access`: acceso de lectura a la definición congelada de una versión publicada concreta.

### Modified Capabilities

- `web-console`: páginas nuevas de credenciales, biblioteca de bloques y versiones (diff/rollback); dashboard con métricas y auto-refresh; acciones de export/import/duplicar en la lista de puentes.

## Impact

- **Backend**: dos endpoints nuevos de solo lectura (`stats`, definición de versión) — nada cambia de contrato; migraciones: ninguna.
- **Consola**: tres páginas nuevas, dashboard ampliado y utilidades de lista; sigue sin haber endpoints de negocio exclusivos de la consola.
- **Compatibilidad**: `add-mvp4-users-roles` queda en espera sin conflicto (este change no toca auth).
- **Fuera de alcance**: edición visual de triggers cron/eventos más allá del JSON actual, notificaciones/alertas, temas claro/oscuro, atajos de teclado.
