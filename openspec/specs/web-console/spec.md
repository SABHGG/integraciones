# web-console Specification

## Purpose
Primera UI propia de la plataforma: una consola web servida por el mismo binario que convierte el plano de administración en un producto utilizable sin curl — dashboard de ejecuciones con trace, gestión visual de puentes y bloques, formularios generados por los metadatos `ui` y editor de código con los errores del validador en línea.

## Requirements

### Requirement: Consola servida por el binario
La consola SHALL seguir siendo una SPA de assets estáticos servida por el mismo binario bajo `/console` que consume exclusivamente el plano de administración existente. El acceso SHALL autenticarse primariamente con **email y contraseña** (token de sesión del plano `auth`); la API key de workspace SHALL seguir aceptada como acceso alternativo de automatización/demo (actúa como `owner`). No SHALL añadirse endpoints de negocio nuevos para la consola.

#### Scenario: Login con email y contraseña
- **WHEN** el usuario introduce credenciales válidas en la pantalla de acceso
- **THEN** la consola obtiene un token de sesión vía el plano `auth` y lo usa como Bearer en todas sus llamadas

#### Scenario: Login con API key
- **WHEN** el usuario introduce una API key de workspace válida en el acceso alternativo
- **THEN** la consola carga el dashboard usando esa key como Bearer (principal de tipo key, rol `owner`)

#### Scenario: Key inválida o sin workspace
- **WHEN** se introduce una API key inexistente o sin workspace asociado
- **THEN** el acceso se rechaza con el error del plano admin y no se carga ningún dato

#### Scenario: Credenciales inválidas
- **WHEN** se introduce un email o contraseña incorrectos
- **THEN** el error mostrado es el `401` uniforme del plano admin y no se carga ningún dato

#### Scenario: Cierre de sesión revoca el token
- **WHEN** el usuario pulsa salir con sesión de usuario
- **THEN** la consola invoca el logout del plano (revocando la sesión en el servidor) y borra el token local

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

### Requirement: UI adaptada al rol
La consola SHALL ocultar o deshabilitar las acciones que el rol del usuario no puede ejecutar (según la matriz del plano): `viewer` navega en lectura; `runner` además dispara, replayea y ensaya; `editor` administra recursos; `owner` además gestiona miembros. Las acciones denegadas por el servidor SHALL mostrarse con el `403` recibido (una sola fuente de verdad: la consola no duplica la matriz).

#### Scenario: Viewer en modo lectura
- **WHEN** entra un `viewer` al editor de puentes
- **THEN** ve la cadena y el trace sin botones de guardar, publicar ni eliminar

#### Scenario: Owner gestiona la membresía
- **WHEN** entra un `owner` a la página de miembros
- **THEN** ve la lista con emails y roles, y puede añadir, cambiar rol y expulsar con los errores del servidor en línea

### Requirement: Página de miembros
La consola SHALL incluir una vista de miembros del workspace (visible íntegra para `owner`, de solo lectura para el resto): lista con email, rol y fecha de alta, y las acciones de la gestión de miembros del plano admin con sus errores visibles.

#### Scenario: Miembro no owner ve la lista sin acciones
- **WHEN** un `editor` abre la página de miembros
- **THEN** ve la lista completa sin controles de edición

### Requirement: Página de credenciales
La consola SHALL incluir una vista de credenciales del workspace: listado con nombre, descripción y versión vigente; creación; rotación (que nunca muestre el valor anterior); eliminación con confirmación; e historial de versiones por credencial (ids y fechas, jamás valores). La vista SHALL respetar el write-only de la bóveda en todas sus superficies. La edición SHALL quedar reservada a `editor`; `viewer` la ve en lectura.

#### Scenario: Rotar desde la consola
- **WHEN** el usuario rota una credencial con un valor nuevo
- **THEN** la consola muestra la nueva versión vigente y el historial lista ambas versiones con sus fechas, sin ningún valor

#### Scenario: Viewer en lectura
- **WHEN** entra un `viewer` a la página de credenciales
- **THEN** ve el listado y el historial (ids y fechas) sin botones de crear, rotar ni eliminar

#### Scenario: Historial sin valores
- **WHEN** se abre el historial de una credencial
- **THEN** la vista muestra ids y fechas; ningún endpoint de la bóveda es invocado de forma que pueda revelar un valor

### Requirement: Biblioteca de bloques
La consola SHALL incluir una vista de la biblioteca de bloques: listado (slug, tipo, categoría, versión vigente), detalle de versiones (schemas y metadatos `ui`), creación de bloques `custom_logic` y edición de su código con el editor con errores del validador en línea, y publicación de versiones con los errores del servidor visibles antes de publicar. Crear, editar y publicar SHALL quedar reservados a `editor`; `viewer` inspecciona la biblioteca en lectura.

#### Scenario: Crear y publicar un bloque custom_logic
- **WHEN** el usuario crea un bloque `custom_logic`, escribe su código TypeScript y publica una versión con un error de transpile
- **THEN** el editor muestra el error con su línea y no se publica; al corregirlo, la versión se publica

#### Scenario: Inspeccionar un bloque built-in
- **WHEN** el usuario abre el detalle de un bloque `http` de la biblioteca base
- **THEN** ve sus versiones con config/input/output schema y los metadatos `ui` que usan los formularios de instancias

#### Scenario: Viewer sin controles de escritura
- **WHEN** entra un `viewer` a la biblioteca de bloques
- **THEN** ve el listado y los detalles sin botones de crear bloque, editar código ni publicar versión

### Requirement: Versiones con diff y rollback
La consola SHALL listar las versiones publicadas de un puente, mostrar un diff visual lado a lado entre dos versiones o entre borrador y versión, y permitir rollback: aplicar la definición de una versión anterior al borrador y publicarla como versión nueva (marcada como tal en la UI), usando la definición congelada del plano. El diff SHALL ser accesible en lectura (`viewer`); el rollback — que publica — SHALL quedar reservado a `editor`.

#### Scenario: Comparar borrador con la versión publicada
- **WHEN** el usuario compara el borrador actual contra la última versión publicada
- **THEN** el diff muestra las diferencias (trigger, bloques, mask, emit, webhook) y permite adoptar la definición publicada al borrador

#### Scenario: Rollback a una versión anterior
- **WHEN** el usuario ejecuta el rollback a la versión 2 de un puente con 5 versiones
- **THEN** se publica una versión 6 con el contenido congelado de la 2 y la UI lo etiqueta como rollback; las versiones 1-5 y sus ejecuciones quedan intactas

#### Scenario: Viewer compara sin rollback
- **WHEN** un `viewer` abre la vista de versiones
- **THEN** ve la lista y el diff sin botón de rollback ni adopción al borrador

### Requirement: Dashboard con métricas y auto-refresh
El dashboard SHALL mostrar las métricas agregadas del workspace (`GET /stats`): total y estados, serie diaria y duración media, con filtro de rango de fechas y desglose por puente navegable al listado filtrado. El listado de ejecuciones SHALL ofrecer auto-refresh por polling configurable (apagado por defecto).

#### Scenario: Salud del workspace de un vistazo
- **WHEN** el usuario abre el dashboard
- **THEN** ve total, tasa de éxito y tendencia diaria del rango elegido, y puede saltar al listado de un puente concreto

#### Scenario: Auto-refresh en vivo
- **WHEN** el usuario activa el auto-refresh en el listado
- **THEN** el listado se refresca periódicamente por polling y el indicador muestra la última actualización; apagado, no hay polling

### Requirement: Export, import y duplicar puentes
La consola SHALL exportar un puente (borrador o una versión publicada) como JSON descargable, importarlo creando un puente nuevo (borrador válido + publicación opcional) y duplicar un puente existente en un clic. El import SHALL validar contra el plano (validación del servidor como única fuente de verdad) y mostrar los errores de validación sobre el formulario. Exportar SHALL ser lectura; importar y duplicar crean recursos, así que SHALL quedar reservados a `editor`.

#### Scenario: Exportar y compartir
- **WHEN** el usuario exporta la versión 3 de un puente
- **THEN** descarga un JSON con la definición congelada (sin ids de workspace) listo para importar en otro workspace

#### Scenario: Import con validación del servidor
- **WHEN** el usuario importa un JSON cuyo borrador tiene una ref rota
- **THEN** la consola crea el puente, ejecuta la validación del servidor y muestra los errores; no publica nada hasta que el borrador es válido

#### Scenario: Viewer exporta pero no crea
- **WHEN** un `viewer` abre la lista de puentes
- **THEN** puede exportar (lectura) pero no ve los botones de importar ni duplicar

### Requirement: Sistema visual El rastro
La consola SHALL aplicar el sistema visual El rastro en todas sus páginas: tokens con regla (papel frío, tinta petróleo; verde/ladrillo/ámbar/azul reservados a estados de ejecución y paso), tipografías autoalojadas sin CDN (grotesca de interfaz, mono solo para valores de máquina) y shell con navegación lateral de tinta como único gesto de identidad. La matriz de roles vigente SHALL preservarse sin cambios.

#### Scenario: Identidad consistente en todas las páginas
- **WHEN** se recorre el login, el shell y las siete superficies de la consola
- **THEN** todas usan los mismos tokens, tipografías y shell, sin estilos ad hoc por página

#### Scenario: Funciona sin internet
- **WHEN** la consola embebida se sirve en una red sin salida a internet
- **THEN** las fuentes locales cargan (con fallback digno) y ninguna petición sale a un CDN

#### Scenario: Foco visible con teclado
- **WHEN** el usuario navega los controles con el teclado
- **THEN** todo control enfocado muestra un anillo de foco visible sobre el sistema de tokens

### Requirement: Rastro en miniatura en el listado de ejecuciones
El listado de ejecuciones SHALL incluir, de forma aditiva, el resumen de pasos de cada ejecución (estado y duración por paso, sin IO) y SHALL renderizarlo como un rastro por fila: un segmento por paso, proporcional a su duración, coloreado por estado y con el total de pasos y duración. El detalle SHALL seguir mostrando el trace completo con IO enmascarado.

#### Scenario: El paso fallido se ve de un vistazo
- **WHEN** el listado muestra una ejecución `failed` de varios pasos
- **THEN** la fila señala con el color de fallo el segmento del paso que falló, sin abrir el detalle

#### Scenario: Pasos condicionales no tomados
- **WHEN** una ejecución incluye pasos `skipped` por su `switch`
- **THEN** sus segmentos aparecen atenuados en su posición de la cadena y el total los cuenta

#### Scenario: Ejecución sin pasos aún
- **WHEN** el listado muestra una ejecución `pending` que aún no tiene pasos
- **THEN** la fila no dibuja segmentos y lo indica con su estado de cola

### Requirement: Estados de vista y uso responsivo
Cada página SHALL tratar carga, vacío y error con mensajes que expliquen qué pasó y qué hacer (los errores del servidor en su forma Problem). La consola SHALL ser operable en móvil sin desbordes. La animación SHALL limitarse a respuestas del usuario y SHALL respetar `prefers-reduced-motion`.

#### Scenario: Vacío que invita
- **WHEN** un workspace nuevo abre la lista de puentes con un rol que puede crear
- **THEN** el estado vacío nombra qué falta y ofrece crear el primer puente

#### Scenario: Móvil operable
- **WHEN** se abre la consola a 390 px de ancho
- **THEN** la navegación y los datos permanecen operables sin desborde horizontal

#### Scenario: Movimiento reducido
- **WHEN** el sistema operativo pide movimiento reducido
- **THEN** la consola no ejecuta animaciones no esenciales

### Requirement: Detalle de bloque con ruta propia
La biblioteca de bloques SHALL abrir el detalle de cada bloque en su propia ruta (`/console/blocks/{id}`): versiones, schemas y edición de `custom_logic` viven en la página del bloque, con el editor a ancho completo y vuelta a la biblioteca. La lista SHALL conservar sus filtros (texto, tipo, categoría) en la URL. Los roles vigentes SHALL respetarse igual que en la vista unificada.

#### Scenario: Volver conserva los filtros
- **WHEN** el usuario filtra la biblioteca, entra al detalle de un bloque y vuelve
- **THEN** la lista muestra el mismo filtro aplicado y su resultado

#### Scenario: URL directa
- **WHEN** se abre `/console/blocks/{id}` sin pasar por la lista
- **THEN** el detalle carga con sus versiones y schemas

#### Scenario: Viewer en lectura
- **WHEN** un `viewer` abre el detalle de un bloque `custom_logic`
- **THEN** ve versiones y schemas sin editor ni botón de publicar

### Requirement: Tour de onboarding en la primera visita
La consola SHALL ofrecer un tour guiado de bienvenida en `/console` para usuarios ya logueados, y SHALL lanzarlo automáticamente la primera vez que ese navegador entra a la consola. El estado "ya lo vi" SHALL persistir en el `localStorage` del navegador, indexado por workspace y por versión del tour, de modo que un navegador con el tour visto no lo reciba de nuevo hasta que cambie la versión del tour o el workspace.

#### Scenario: Primera visita lanza el tour
- **WHEN** un usuario logueado entra a la consola por primera vez en ese navegador
- **THEN** el tour arranca automáticamente tras cargarse el shell, sin intervención del usuario

#### Scenario: Visitas posteriores no relanzan el tour
- **WHEN** el mismo usuario vuelve a entrar a la consola en el mismo navegador y workspace
- **THEN** el tour no arranca solo, porque el navegador ya lo tiene registrado como visto

#### Scenario: Nueva versión del tour lo relanza
- **WHEN** la versión declarada del tour cambia y el usuario entra a la consola
- **THEN** el tour vuelve a lanzarse automáticamente aunque el navegador tuviera la versión anterior vista

### Requirement: Relanzar el tour bajo demanda
La consola SHALL exponer un botón "Ver tutorial" permanente en el shell (visible junto a la navegación, en escritorio y en móvil) que relanza el tour desde el principio. Pulsarlo SHALL funcionar con independencia del estado guardado, sin consumir ni exigir un recurso del servidor.

#### Scenario: Relanzar tras completarlo
- **WHEN** el usuario ya vio el tour y pulsa "Ver tutorial"
- **THEN** el tour arranca de nuevo desde el primer paso

#### Scenario: Relanzar sin paso previo por el tour
- **WHEN** un usuario cuyo navegador no tiene el tour registrado pulsa "Ver tutorial"
- **THEN** el tour arranca igualmente y al terminar queda registrado como visto

### Requirement: Pasos del tour filtrados por rol
Los pasos del tour SHALL filtrarse según el rol de la sesión (`viewer`, `runner`, `editor`, `owner`), en coherencia con la UI por rol: los pasos que explican superficies reservadas a un rol superior SHALL omitirse para roles inferiores (por ejemplo, Miembros solo para `owner` y edición solo para `editor` o superior), mientras que los pasos comunes (dashboard y trace) SHALL aparecer para todos los roles.

#### Scenario: Viewer sin pasos de gestión
- **WHEN** un `viewer` completa el tour
- **THEN** el tour no incluye pasos sobre gestión de miembros ni sobre crear, editar o publicar

#### Scenario: Owner con el recorrido completo
- **WHEN** un `owner` completa el tour
- **THEN** el tour incluye el recorrido completo, incluidos los pasos de miembros y de edición

#### Scenario: Pasos comunes para todos los roles
- **WHEN** cualquier rol completa el tour
- **THEN** recorre los pasos del dashboard de ejecuciones y del trace, que son comunes

### Requirement: Tour sin dependencias de backend
El tour SHALL funcionar íntegramente en el navegador y SHALL NO introducir endpoints, tablas ni recursos nuevos en el plano admin. Su biblioteca de tour SHALL proveerse como dependencia empaquetada con la consola y SHALL NO cargarse desde una CDN ni requerir salida a internet.

#### Scenario: Sin peticiones por el tour
- **WHEN** el tour se lanza, avanza y termina
- **THEN** ninguna petición al backend se origina por el tour, ni para leerlo ni para marcar su estado

#### Scenario: Funciona sin internet
- **WHEN** la consola se sirve en una red sin salida a internet y el tour se lanza
- **THEN** el tour funciona con la biblioteca empaquetada localmente, sin peticiones a una CDN

### Requirement: Presentación del tour en el sistema El rastro
El popover y el resaltado del tour SHALL mapearse a los tokens de El rastro (papel, tinta, superficies y bordes) y SHALL respetar el contraste AA y los tokens tipográficos vigentes. El tour SHALL respetar `prefers-reduced-motion`: con movimiento reducido solicitado por el sistema, SHALL evitar transiciones no esenciales al moverse entre pasos.

#### Scenario: Popover con los tokens del sistema
- **WHEN** el tour muestra un paso
- **THEN** el popover usa los colores, bordes y tipografías de El rastro, sin colores ad hoc fuera del sistema

#### Scenario: Movimiento reducido respetado
- **WHEN** el sistema operativo pide movimiento reducido y el tour avanza entre pasos
- **THEN** el tour no ejecuta transiciones no esenciales

### Requirement: Degradación segura y limpieza del tour
El tour SHALL filtrar cualquier paso cuyo elemento destino no exista en el momento de lanzarse, en lugar de fallar o mostrar un paso sin destino. La instancia del tour SHALL destruirse limpiamente al navegar a otra ruta o al desmontar el shell, de modo que no deje popovers, resaltados, manejadores de teclado ni estado residual.

#### Scenario: Selector ausente no rompe el tour
- **WHEN** un paso apunta a un elemento que no está presente en la página en ese momento
- **THEN** ese paso se omite y el tour continúa con el resto de los pasos aplicables

#### Scenario: Navegar durante el tour lo limpia
- **WHEN** el usuario navega a otra página o desmonta el shell con el tour abierto
- **THEN** la instancia del tour se destruye y no quedan popovers, resaltados ni manejadores activos
