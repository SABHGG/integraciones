# web-console Delta

## ADDED Requirements

### Requirement: Sistema visual El rastro
La consola SHALL aplicar el sistema visual El rastro en todas sus páginas: tokens con regla (papel frío, tinta petróleo; verde/ladrillo/ámbar/azul reservados a estados de ejecución y paso), tipografías autoalojadas sin CDN (grotesca de interfaz, mono solo para valores de máquina) y shell con navegación lateral de tinta como único gesto de identidad. La matriz de roles vigente SHALL preservarse sin cambios.

#### Scenario: Identidad consistente en todas las páginas
- **WHEN** se recorre el login, el shell y las siete superficies de la consola
- **THEN** todas usan los mismos tokens, tipografías y shell, sin estilos ad hoc por página

#### Scenario: Funciona sin internet
- **WHEN** la consola embebida se sirve en una red sin salida a internet
- **THEN** las fuentes locales cargan (con fallback digno) y ninguna petición sale a un CDN

#### Scenario: Foco visible con teclado
- **WHEN** el usuario navega los controles con el teclado
- **THEN** todo control enfocado muestra un anillo de foco visible sobre el sistema de tokens

### Requirement: Rastro en miniatura en el listado de ejecuciones
El listado de ejecuciones SHALL incluir, de forma aditiva, el resumen de pasos de cada ejecución (estado y duración por paso, sin IO) y SHALL renderizarlo como un rastro por fila: un segmento por paso, proporcional a su duración, coloreado por estado y con el total de pasos y duración. El detalle SHALL seguir mostrando el trace completo con IO enmascarado.

#### Scenario: El paso fallido se ve de un vistazo
- **WHEN** el listado muestra una ejecución `failed` de varios pasos
- **THEN** la fila señala con el color de fallo el segmento del paso que falló, sin abrir el detalle

#### Scenario: Pasos condicionales no tomados
- **WHEN** una ejecución incluye pasos `skipped` por su `switch`
- **THEN** sus segmentos aparecen atenuados en su posición de la cadena y el total los cuenta

#### Scenario: Ejecución sin pasos aún
- **WHEN** el listado muestra una ejecución `pending` que aún no tiene pasos
- **THEN** la fila no dibuja segmentos y lo indica con su estado de cola

### Requirement: Estados de vista y uso responsivo
Cada página SHALL tratar carga, vacío y error con mensajes que expliquen qué pasó y qué hacer (los errores del servidor en su forma Problem). La consola SHALL ser operable en móvil sin desbordes. La animación SHALL limitarse a respuestas del usuario y SHALL respetar `prefers-reduced-motion`.

#### Scenario: Vacío que invita
- **WHEN** un workspace nuevo abre la lista de puentes con un rol que puede crear
- **THEN** el estado vacío nombra qué falta y ofrece crear el primer puente

#### Scenario: Móvil operable
- **WHEN** se abre la consola a 390 px de ancho
- **THEN** la navegación y los datos permanecen operables sin desborde horizontal

#### Scenario: Movimiento reducido
- **WHEN** el sistema operativo pide movimiento reducido
- **THEN** la consola no ejecuta animaciones no esenciales
