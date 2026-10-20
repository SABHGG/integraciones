# Tasks

## 1. Ruta y página de detalle

- [ ] 1.1 Ruta `/blocks/{id}` en `main.tsx` (pantalla `bloque`, sección Bloques activa en el menú)
- [ ] 1.2 `DetalleBloque` como página: volver a la biblioteca (conservando query), descripción, versiones, input schema y editor de `custom_logic` con publicación — misma lógica del panel embebido
- [ ] 1.3 La lista navega al detalle (clic y Enter) y pierde el panel embebido y su estado

## 2. Filtros de la lista en la URL

- [ ] 2.1 Leer `q`/`tipo`/`cat` de la query al montar y escribir cambios con `history.replaceState`

## 3. Verificación

- [ ] 3.1 Ida y vuelta conserva filtros; URL directa de detalle carga sin pasar por la lista; `viewer` ve todo sin controles de edición
- [ ] 3.2 Build + Vitest verdes y capturas 1440/390 de lista y detalle
