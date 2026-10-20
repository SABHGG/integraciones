# Tasks

## 1. Ruta y página de detalle

- [x] 1.1 Ruta `/blocks/{id}` en `main.tsx` (pantalla `bloque`, sección Bloques activa en el menú)
- [x] 1.2 `DetalleBloque` como página: volver a la biblioteca (conservando query), descripción, versiones, input schema y editor de `custom_logic` con publicación — misma lógica del panel embebido
- [x] 1.3 La lista navega al detalle (clic y Enter) y pierde el panel embebido y su estado

## 2. Filtros de la lista en la URL

- [x] 2.1 Leer `q`/`tipo`/`cat` de la query al montar y escribir cambios con `history.replaceState`

## 3. Verificación

- [x] 3.1 Ida y vuelta conserva filtros; URL directa de detalle carga sin pasar por la lista; `viewer` ve todo sin controles de edición
- [x] 3.2 Build + Vitest verdes y capturas 1440/390 de lista y detalle
