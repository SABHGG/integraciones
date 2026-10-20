# web-console Delta

## ADDED Requirements

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
