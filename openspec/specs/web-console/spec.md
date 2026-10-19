# web-console Specification

## Purpose
Primera UI propia de la plataforma: una consola web servida por el mismo binario que convierte el plano de administración en un producto utilizable sin curl — dashboard de ejecuciones con trace, gestión visual de puentes y bloques, formularios generados por los metadatos `ui` y editor de código con los errores del validador en línea.

## Requirements

### Requirement: Consola servida por el binario
El sistema SHALL servir la consola web como assets estáticos desde el mismo binario del api, como SPA que consume exclusivamente el plano de administración existente (mismos endpoints, mismo Bearer). El acceso SHALL autenticarse con una API key de workspace introducida en el login; no SHALL añadirse endpoints de negocio nuevos para la consola.

#### Scenario: Login con API key
- **WHEN** el usuario introduce una API key de workspace válida en la pantalla de acceso
- **THEN** la consola carga el dashboard usando esa key como Bearer en todas sus llamadas

#### Scenario: Key inválida o sin workspace
- **WHEN** se introduce una API key inexistente o sin workspace asociado
- **THEN** el acceso se rechaza con el error del plano admin y no se carga ningún dato

### Requirement: Dashboard de ejecuciones con trace
La consola SHALL presentar el listado de ejecuciones con filtros por puente, estado y rango de fechas (paginación por cursor), el detalle de cada ejecución y un visor de trace paso a paso — incluyendo los pasos `skipped` — con el IO (ya enmascarado) y el punto exacto del fallo.

#### Scenario: Diagnosticar un fallo hasta el bloque exacto
- **WHEN** el usuario abre una ejecución failed desde el dashboard
- **THEN** el visor señala el paso que falló con su error y permite inspeccionar el input que recibió

#### Scenario: Los pasos condicionales se ven como skipped
- **WHEN** el trace de un puente con `switch` muestra rutas no tomadas
- **THEN** el visor las presenta como pasos `skipped` en su posición de la cadena

### Requirement: Formularios generados por metadatos ui
Los formularios de configuración de instancias SHALL generarse automáticamente desde el campo `ui` de la versión del bloque (tipos, etiquetas, valores por defecto y validación de cliente); un bloque sin metadatos `ui` SHALL recibir un formulario JSON genérico.

#### Scenario: Formulario derivado de los metadatos
- **WHEN** el usuario añade una instancia del bloque `http-get` a un puente
- **THEN** el formulario muestra sus campos declarados en `ui` (url, headers, timeout…) con su validación

#### Scenario: Bloque sin ui metadatos
- **WHEN** un bloque `custom_logic` se publica sin campo `ui`
- **THEN** su configuración se edita como JSON libre con validación contra inputSchema al guardar

### Requirement: Editores con validación en línea
La consola SHALL incluir un editor de código (TS/JS) para bloques `custom_logic` y un editor visual de la cadena del puente (orden lineal, `when` y rutas de `switch`): los errores de publicación del servidor (transpile, schemas, rutas hacia atrás, refs imposibles) SHALL mostrarse sobre el editor antes de permitir publicar.

#### Scenario: Error de TypeScript visible al publicar
- **WHEN** el usuario publica un bloque con TypeScript que no transpila
- **THEN** la consola muestra el error del validador junto a la línea correspondiente y no publica

#### Scenario: Ruta de switch inválida marcada en el editor visual
- **WHEN** una ruta del `switch` apunta a una instancia anterior en la cadena
- **THEN** el editor visual la marca como error de validación y la publicación queda bloqueada
