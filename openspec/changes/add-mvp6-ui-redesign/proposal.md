# Proposal

## Why

La consola creció por acumulación: cinco hitos de funcionalidad sobre una hoja CSS artesanal (~180 líneas) que define el mínimo para operar, pero no una identidad. El resultado es un esqueleto utilitario — jerarquía débil, sin estados de carga o vacío cuidados, sin responsive real y con cada página inventando su propio acomodo. Este hito dedica un MVP completo a la capa de UI: dotar a la consola de un sistema visual propio, verificable y consistente, **sin cambiar nada de comportamiento** — toda la funcionalidad del MVP3+MVP5 y la matriz de roles del MVP4 quedan intactas.

La dirección visual fue elegida por el propietario tras una ronda de conceptos (A: sala de control oscura, B: papel operativo, C: patch-bay, D: el rastro) usando los playbooks ui-taste (image-first, anti-slop) y frontend-design (anti-tells de diseño generado). **D — "El rastro"** es la elegida: la firma del sistema es el artefacto más característico del producto, el rastro de ejecución de un puente.

## What Changes

- **Sistema visual "El rastro"**: tokens con regla (papel frío + tinta petróleo; el color solo significa estado), tipografías autoalojadas sin CDN (grotesca para interfaz, mono solo para valores de máquina), shell con navegación lateral de tinta como único gesto audaz, banda de métricas con divisores en lugar de tarjetas. Target congelado en `target/` (captura 1440×900 verificada en Chromium).
- **Rastro en miniatura en el listado de ejecuciones**: cada fila muestra un segmento por paso, proporcional a su duración y coloreado por estado — ver qué bloque es lento o falló sin abrir el detalle. Requiere un cambio aditivo de solo lectura: el listado `GET /executions` incluye el resumen de pasos (estado y duración, **sin IO**).
- **Estados de vista completos**: carga, vacío y error con mensajes que explican qué pasó y qué hacer, en todas las páginas.
- **Responsive estructural**: navegación utilizable en móvil, datos que apilan sin desbordes; movimiento solo como respuesta al usuario y con `prefers-reduced-motion`; foco visible en todos los controles.
- **Tailwind v4 como base de estilos**: tokens en `@theme`, reemplazo progresivo de `styles.css`; dependencia de build, sin runtime ni CDN.

## Capabilities

### Modified Capabilities

- `web-console`: sistema visual El rastro obligatorio en todas las páginas; rastro en miniatura en el listado de ejecuciones (con el soporte aditivo del plano admin); estados de carga/vacío/error y uso responsivo. Los requisitos funcionales existentes (dashboard, formularios, editores, roles, páginas, versiones, métricas, export/import) no cambian.

## Impact

- **Backend**: un único cambio aditivo de lectura — el listado de ejecuciones añade el resumen de pasos por ejecución. Sin migraciones, sin endpoints nuevos, sin cambios de contrato existente (solo un campo nuevo en la respuesta).
- **Consola**: reconstrucción visual completa del shell y las siete superficies con Tailwind v4; los componentes y las llamadas API existentes se conservan.
- **Compatibilidad**: el espejo `puede()` de roles sigue siendo solo presentación; el servidor sigue siendo la única fuente de verdad (403 en línea). Las fuentes se autoalojan: la consola embebida sigue funcionando sin internet.
- **Fuera de alcance**: temas claro/oscuro, canvas visual de puentes, notificaciones o alertas, atajos de teclado, internacionalización.
