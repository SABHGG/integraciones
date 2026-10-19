# bridge-version-access Specification

## Purpose

Acceso de lectura a la definición congelada de una versión publicada concreta, para diff y rollback sin tocar el pasado.

## Requirements

### Requirement: Definición de una versión publicada
El sistema SHALL exponer `GET /api/v1/bridges/{id}/versions/{version}` (plano admin; lectura según la matriz de roles — `viewer` puede leer) que devuelve los metadatos de la versión (`id`, `version`, `createdAt`) y su `definition` congelada exactamente como se publicó. La versión SHALL referirse a una versión existente del puente (404 en caso contrario). El endpoint SHALL ser de solo lectura: ninguna operación de escritura nueva.

#### Scenario: Definición congelada recuperable
- **WHEN** se solicita la versión 3 de un puente con tres versiones publicadas
- **THEN** la respuesta devuelve la definición exacta con la que corrieron las ejecuciones de esa versión (trigger, blocks con code, mask, emit, webhook)

#### Scenario: Versión inexistente
- **WHEN** se solicita una versión que el puente no tiene (p. ej. la 9 con solo 3 publicadas)
- **THEN** la respuesta es `404` Problem Details

#### Scenario: Rollback por re-publicación
- **WHEN** la consola toma la definición de una versión anterior, la aplica al borrador y publica
- **THEN** se crea una versión NUEVA con ese contenido; ninguna versión previa ni ejecución histórica cambia (semántica de inmutabilidad vigente)
