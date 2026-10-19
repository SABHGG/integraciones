# Spec Delta: credentials-vault

## REMOVED Requirements

### Requirement: Disponibilidad mínima en MVP1

## ADDED Requirements

### Requirement: Versionado y rotación de secretos
El sistema SHALL versionar cada credencial: toda escritura del valor (`POST`/`PATCH`) SHALL crear una versión inmutable cifrada con su fecha de rotación, la versión más reciente SHALL ser la vigente que resuelve `ctx.credential(name)`, y las ejecuciones SHALL registrar qué versión de cada credencial usaron (identificador de versión, jamás el valor). El listado SHALL exponer el historial de versiones como metadatos, nunca valores.

#### Scenario: Rotar sin tocar los puentes
- **WHEN** se rota el valor de una credencial con `PATCH` creando la versión 2
- **THEN** las siguientes ejecuciones de todos los puentes que la referencian resuelven la versión 2 sin cambios en ninguna definición

#### Scenario: Auditoría de la versión usada
- **WHEN** se consulta el detalle de una ejecución anterior a una rotación
- **THEN** la ejecución registra el id de la versión de credencial que usó, verificable contra el historial

#### Scenario: El historial no revela valores
- **WHEN** se consulta el historial de versiones de una credencial
- **THEN** se listan metadatos (ids, fechas de rotación) y ninguna versión devuelve su valor
