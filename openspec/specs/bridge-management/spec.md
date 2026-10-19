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
El sistema SHALL almacenar la configuración de disparo (HTTP: method+path; cron: expresión+timezone; evento: topic) dentro del borrador del puente y conservarla en el snapshot de cada versión publicada. El slug de invocación SHALL ser único por workspace. Con este change el trigger cron pasa a estar soportado de verdad: schedule y timezone se validan al publicar.

#### Scenario: Slug único y estable
- **WHEN** se publica un puente con slug `facturar-pedido`
- **THEN** el plano de invocación expone `/x/facturar-pedido` según el method declarado en el trigger
- **AND** otro puente del mismo workspace no puede reclamar el mismo slug

#### Scenario: Trigger cron validado al publicar
- **WHEN** se publica un puente con `{type: "cron", schedule: "0 9 * * 1-5", timezone: "Europe/Madrid"}`
- **THEN** la publicación procede y la versión congela schedule y timezone ejecutables

#### Scenario: Trigger cron inválido bloquea la publicación
- **WHEN** el trigger declara una expresión que no es cron de 5 campos o una timezone IANA desconocida
- **THEN** la publicación falla con 422 Problem Details antes de crear la versión

### Requirement: Enrutado condicional en la definición
La definición del puente SHALL admitir enrutado condicional: cada instancia puede declarar `when` (condición que decide si se ejecuta) y la cadena puede incluir un bloque `switch` con rutas nombradas hacia instancias posteriores por su id (con ruta `default` opcional). La validación al publicar SHALL comprobar que toda ruta apunta a una instancia existente y posterior, sin saltos hacia atrás, y SHALL rechazar condiciones no declarativas.

#### Scenario: Switch con rutas válidas publica
- **WHEN** un puente declara un `switch` con rutas `vip → instancia "premium"` y `default → instancia "standard"`, ambas posteriores al switch
- **THEN** la publicación procede y ambas rutas quedan congeladas en el snapshot

#### Scenario: Un salto hacia atrás bloquea la publicación
- **WHEN** una ruta del switch apunta a una instancia anterior en la cadena (formaría un ciclo)
- **THEN** la publicación falla con 422 Problem Details identificando la ruta que salta hacia atrás

#### Scenario: Condición no determinista rechazada
- **WHEN** un `when` declara código JavaScript arbitrario en lugar de la gramática de condiciones
- **THEN** la publicación falla con 422 Problem Details indicando que las condiciones son declarativas
