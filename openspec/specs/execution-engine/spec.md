# Execution Engine Specification

## Purpose

Motor de ejecución: recorre la cadena lineal de bloques de un puente en un isolate QuickJS fresco por ejecución, resuelve referencias `$.`, valida esquemas, aplica fail-fast y persiste la entrada/salida de cada paso. Desacoplado del transporte HTTP: el disparo solo encola.

## Requirements

### Requirement: Sandbox aislado por ejecución
El sistema SHALL ejecutar cada bloque en un isolate QuickJS (rquickjs + quickjs-ng) creado fresco para la ejecución y destruido al terminar, con límite de memoria configurable y un interrupt handler que corta la ejecución al vencer el timeout del puente definido en milisegundos.

#### Scenario: Bucle infinito es interrumpido
- **WHEN** el código de un bloque entra en un bucle infinito y el timeout del puente es de 5000 ms
- **THEN** el interrupt del isolate corta la ejecución cerca del límite
- **AND** la ejecución termina en estado failed con error `TIMEOUT` y el worker queda disponible para la siguiente ejecución

#### Scenario: El sandbox no deja estado residual
- **WHEN** una ejecución define variables globales y termina
- **THEN** la siguiente ejecución arranca con un isolate nuevo sin acceso al estado previo

### Requirement: Ejecutor desacoplado del transporte
El ejecutor SHALL ser un componente independiente del transporte HTTP que recibe `{definición de versión, entrada}` y produce `{resultado, trace}` persistido en el store. El mismo ejecutor SHALL servir al disparo HTTP sync (MVP1) y a los disparadores futuros (async, cron, eventos).

#### Scenario: Transporte y ejecución independientes
- **WHEN** el endpoint HTTP recibe una invocación válida
- **THEN** delega al ejecutor y construye la respuesta a partir del resultado persistido
- **AND** ningún código de ejecución de bloques vive dentro del handler de request/response

### Requirement: Cadena lineal con fail-fast
El ejecutor SHALL recorrer los bloques instanciados en orden de cadena: resolver el input de cada bloque desde el contexto (referencias `$.`), validarlo contra el inputSchema del bloque, invocar el sandbox y registrar la salida nombrada. Una instancia con `when` cuya condición evalúa a falso SHALL registrarse como `skipped` sin invocar el sandbox ni producir salida; un bloque `switch` SHALL evaluar sus condiciones en orden de declaración, saltar a la primera ruta verdadera (o a `default` si existe) y registrar las rutas no tomadas como `skipped`. En la ruta efectivamente tomada, el primer error SHALL terminar la ejecución en estado failed con el error registrado en el trace.

#### Scenario: Error de bloque detiene la cadena
- **WHEN** el segundo bloque de una cadena de tres lanza `throw`
- **THEN** la ejecución termina failed, el tercer bloque no se invoca
- **AND** el trace registra input/output/duración de los bloques 1 y 2 y el error exacto del 2

#### Scenario: Violación de schema detectada antes de ejecutar
- **WHEN** la salida resuelta de un bloque no cumple el inputSchema del siguiente
- **THEN** la ejecución termina failed con error `VALIDATION_FAILED` identificando el bloque y el campo, sin invocar el sandbox

#### Scenario: Un when falso marca el paso como skipped
- **WHEN** una instancia declara `when` que evalúa a falso sobre el contexto
- **THEN** el paso queda registrado como `skipped` sin input ejecutado ni salida, y la cadena continúa con el siguiente bloque

#### Scenario: Un switch enruta por la primera condición verdadera
- **WHEN** el switch tiene rutas `vip` y `standard` y la condición de `vip` evalúa a verdadera
- **THEN** el ejecutor salta a la instancia de la ruta `vip`, la ruta `standard` queda `skipped` y ninguna otra condición del switch se evalúa

#### Scenario: Switch sin ruta verdadera usa default
- **WHEN** ninguna condición del switch evalúa a verdadera y existe ruta `default`
- **THEN** el ejecutor salta a la instancia de `default` y registra el resto de rutas como `skipped`

### Requirement: Pipeline TypeScript
El sistema SHALL transpilar TypeScript a JavaScript al publicar la versión del bloque (swc); el runtime SHALL recibir únicamente JS ya compilado.

#### Scenario: Error de sintaxis detectado al publicar
- **WHEN** se publica una versión de bloque con TypeScript inválido
- **THEN** la publicación falla en el momento de la publicación y ninguna ejecución llega a ver el código roto

### Requirement: Condiciones deterministas sobre el contexto
Las condiciones (`when` del switch) SHALL evaluarse en el host con una gramática declarativa sobre referencias `$.`: comparaciones (`eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `exists`, `contains`) combinables con `and`/`or`/`not`, sin ejecutar código del usuario para decidir rutas. Una referencia no resuelta dentro de una condición SHALL producir fail-fast con `REF_UNRESOLVED`, igual que en los mapeos de entrada.

#### Scenario: Condición compuesta evaluada en el host
- **WHEN** un `when` declara `{and: [{gte: ["$.cliente.pedidos", 10]}, {eq: ["$.cliente.pais", "ES"]}]}` y ambos términos se cumplen
- **THEN** la instancia se ejecuta sin que el sandbox participe en la decisión

#### Scenario: Referencia no resuelta en una condición es fail-fast
- **WHEN** una condición referencia `$.cliente.pais` y el bloque origen no produjo `cliente`
- **THEN** la ejecución termina failed con `REF_UNRESOLVED` identificando la condición y la referencia
