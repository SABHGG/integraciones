# Bridge Management Specification

## Purpose

Gestión del ciclo de vida de los puentes: borrador editable, validación en tiempo de diseño (refs `$.`, schemas, bloques resueltos) y publicación de versiones inmutables que congelan un snapshot ejecutable. El trigger HTTP forma parte de la definición del puente.

## Requirements

### Requirement: Puentes con borrador y versiones inmutables
El sistema SHALL permitir crear y editar puentes como borrador (`bridges`) y publicar versiones inmutables (`bridge_versions`, `UNIQUE(bridge_id, version)`) cuyo snapshot JSONB contiene la definición completa: cadena de bloques instanciados, mapeos de entrada, configuraciones, trigger e inputSchema.

#### Scenario: Publicar un puente válido
- **WHEN** se publica `POST /api/v1/bridges/{id}/versions` de un borrador que pasa la validación
- **THEN** el sistema crea una versión inmutable con snapshot completo de la definición
- **AND** las ejecuciones posteriores corren contra esa versión aunque el borrador siga editándose

#### Scenario: Las ejecuciones históricas conservan su definición
- **WHEN** un puente se edita y republica después de tener ejecuciones registradas
- **THEN** las ejecuciones previas conservan la referencia a la versión con la que corrieron y su trace es interpretable

### Requirement: Validación en tiempo de diseño al publicar
El sistema SHALL validar al publicar: (a) estructura — cadena lineal con un único bloque de entrada y sin bloques huérfanos; (b) compatibilidad de schemas — cada campo mapeado del input debe ser compatible en tipo con el outputSchema del bloque origen cuando el schema define la ruta; (c) referencias — toda referencia `$.` verificable e inexistente es error de publicación; rutas no verificables (schema suelto) se permiten sin aviso.

#### Scenario: Referencia imposible bloquea la publicación
- **WHEN** un mapeo referencia `$.cliente.output.email` y el outputSchema del bloque `cliente` no declara `email`
- **THEN** la publicación falla con 422 Problem Details detallando la referencia inválida

#### Scenario: Schema suelto no bloquea
- **WHEN** el outputSchema del bloque origen no permite verificar la ruta referenciada
- **THEN** la publicación procede sin errores ni advertencias

### Requirement: Sintaxis de referencias del contexto
El sistema SHALL tratar un string como referencia si y solo si empieza con `$.`, con raíces reservadas `$.trigger` y `$.now`, segmentos numéricos para arrays y resolución a cualquier nodo. La misma gramática SHALL estar disponible como interpolación en strings de configuración (ej. URLs: `https://api.x.com/users/{$.auth.userId}`). Toda referencia no resuelta en runtime SHALL producir fail-fast con mensaje que identifique la referencia y el bloque.

#### Scenario: Encadenamiento sin arrastre de campos
- **WHEN** un bloque intermedio no mapea campos producidos por bloques previos
- **THEN** un bloque posterior puede referenciar directamente `$.primerBloque.campo` sin que los intermedios lo arrastren

### Requirement: Trigger como parte del puente
El sistema SHALL almacenar la configuración de disparo (HTTP: method+path; cron: expresión+timezone; evento: topic) dentro del borrador del puente y conservarla en el snapshot de cada versión publicada. El slug de invocación SHALL ser único por workspace.

#### Scenario: Slug único y estable
- **WHEN** se publica un puente con slug `facturar-pedido`
- **THEN** el plano de invocación expone `/x/facturar-pedido` según el method declarado en el trigger
- **AND** otro puente del mismo workspace no puede reclamar el mismo slug
