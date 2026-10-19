# dry-run Specification

## Purpose
Ensayo del puente sin efectos de producto: ejecutar el borrador con datos de prueba y salidas mock por instancia para validar la cadena antes de publicar, sin crear ejecuciones ni disparar integraciones de resultado.

## Requirements

### Requirement: Dry-run del borrador
El sistema SHALL exponer `POST /api/v1/bridges/{id}/dry-run` (plano de administración) que ejecuta el borrador actual del puente — sin requerir versión publicada — con las mismas reglas del ejecutor (resolución de refs `$.`, inputSchema, fail-fast, timeout) y devuelve el trace completo (pasos con IO, duración y estado) en la propia respuesta.

#### Scenario: Dry-run exitoso devuelve el trace
- **WHEN** se lanza un dry-run con datos de prueba sobre un borrador válido
- **THEN** la respuesta incluye el resultado final y los pasos con su IO y duración, como un trace de ejecución real

#### Scenario: El borrador no necesita estar publicado
- **WHEN** se lanza un dry-run sobre un puente que nunca se ha publicado
- **THEN** el ensayo corre contra el borrador actual y no exige versión publicada

### Requirement: Mocks por instancia
En un dry-run, cada instancia SHALL poder declararse mock con una salida fija (`mock: {output}`): el bloque mockeado NO SHALL invocarse (ni sandbox ni HTTP) y su salida declarada SHALL alimentar el contexto como si hubiera corrido. Las instancias sin mock SHALL ejecutarse de verdad (con sus credenciales y llamadas reales).

#### Scenario: Un HTTP mockeado evita la llamada real
- **WHEN** el dry-run marca la instancia `traeUsuario` con `mock: {output: {usuario: {nombre: "Ana"}}}`
- **THEN** el bloque no llama a la API externa y el siguiente bloque recibe `{usuario: {nombre: "Ana"}}`

#### Scenario: Cadena mixta de mocks y bloques reales
- **WHEN** solo el primer bloque de tres está mockeado
- **THEN** el segundo y el tercero ejecutan su código real consumiendo la salida del mock

### Requirement: El dry-run no deja rastro de ejecución
Un dry-run SHALL NOT crear filas en `executions`/`execution_steps`, SHALL NOT disparar webhooks y SHALL NOT emitir eventos; el resultado vive únicamente en la respuesta del endpoint.

#### Scenario: El listado de ejecuciones no cambia tras un dry-run
- **WHEN** se lanza un dry-run y a continuación se consulta `GET /api/v1/executions`
- **THEN** no aparece ninguna ejecución nueva atribuible al dry-run

#### Scenario: Un fallo del dry-run no dispara integraciones
- **WHEN** el dry-run termina en fallo de un puente con webhook y `emit` configurados
- **THEN** no se entrega webhook alguno ni se publica evento
