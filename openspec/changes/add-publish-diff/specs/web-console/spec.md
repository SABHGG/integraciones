# Spec Delta

## ADDED Requirements

### Requirement: Confirmación de publicación con diff
Al publicar una versión, la consola SHALL mostrar en la confirmación el diff estructural entre el borrador y la última versión publicada — resumen (`resumenDiff`) y lista plegable de líneas `ruta: izq → der` — antes de ejecutar la publicación irreversible. El diff SHALL limitarse a las secciones editables del borrador.

#### Scenario: Confirmación con diferencias
- **WHEN** el usuario modifica el borrador (p. ej. cambia el `config.url` del bloque 1) y pulsa Publicar versión
- **THEN** la confirmación muestra «N diferencias» y las líneas exactas (`blocks.1.config.url`, valor viejo → nuevo) junto al resumen orgánico, antes de «Publicar ahora»

#### Scenario: Borrador idéntico a la última versión
- **WHEN** el borrador es idéntico a la última versión publicada y el usuario pulsa Publicar
- **THEN** la confirmación muestra «idénticos» y la publicación sigue disponible

#### Scenario: Primera publicación
- **WHEN** el puente no tiene ninguna versión publicada y el usuario pulsa Publicar
- **THEN** la confirmación dice «primera versión — no hay una anterior con la que comparar» y omite el diff, sin bloquear la publicación

#### Scenario: Sin ruido del servidor
- **WHEN** la definición congelada difiere del borrador en rutas fuera de las secciones editables
- **THEN** esas rutas no aparecen en el diff de la confirmación

#### Scenario: Definición anterior no cargada
- **WHEN** la petición de la definición de la última versión falla y el usuario confirma la publicación
- **THEN** la confirmación indica que no se pudo comparar y permite publicar con el resumen orgánico
