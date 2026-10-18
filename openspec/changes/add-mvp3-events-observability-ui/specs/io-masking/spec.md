# Spec Delta: io-masking

## Purpose

Enmascaramiento declarativo del IO observable: el autor del puente declara qué campos del input/salida son sensibles y el sistema los sustituye de forma irreversible al persistir, de modo que traces, listados y webhooks pueden consultarse sin exponer datos de clientes.

## ADDED Requirements

### Requirement: Reglas de enmascaramiento en la definición
La definición del puente SHALL admitir reglas declarativas `mask: [paths]` (rutas `$.` sobre el IO de los pasos o sobre el sobre final, congeladas en el snapshot): al persistir cada paso, los campos cubiertos SHALL sustituirse por `***` de forma irreversible, sin conservar el valor original en ninguna tabla.

#### Scenario: Campo enmascarado en el trace
- **WHEN** un puente declara `mask: ["$.cliente.tarjeta"]` y el bloque produce ese campo
- **THEN** el trace persiste `cliente.tarjeta: "***"` y el resto del IO íntegro

#### Scenario: El enmascaramiento es irreversible
- **WHEN** un administrador consulta cualquier endpoint de ejecuciones de una ejecución enmascarada
- **THEN** el valor original no es recuperable por ningún medio: no existe flag ni endpoint de revelado

### Requirement: Todas las superficies entregan el dato enmascarado
El enmascaramiento SHALL aplicarse antes de la persistencia, de modo que trace, detalle, listado y la entrega webhook muestren el dato ya enmascarado; un cambio de reglas SHALL afectar solo a ejecuciones posteriores (las versiones conservan sus reglas congeladas).

#### Scenario: El webhook recibe el dato enmascarado
- **WHEN** una ejecución de puente con mask y webhook termina con éxito
- **THEN** el sobre entregado por webhook contiene `***` en los campos enmascarados

#### Scenario: Editar reglas no reescribe el pasado
- **WHEN** se añade una regla de mask al borrador y se publica una versión nueva
- **THEN** los traces de versiones anteriores conservan su IO tal como se persistió
