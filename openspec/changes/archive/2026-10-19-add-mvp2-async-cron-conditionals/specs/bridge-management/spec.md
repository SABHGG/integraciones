# Spec Delta: bridge-management

## MODIFIED Requirements

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

## ADDED Requirements

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
