# web-console Delta

## ADDED Requirements

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
