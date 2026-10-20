# Tasks

## 1. Dependencia y lógica pura del tour

- [ ] 1.1 Instalar `driver.js` como dependencia npm de la consola (`npm install driver.js` en `console/`) y verificar que aparece en `console/package.json` y en el lockfile; confirmar que no se añade ninguna referencia a CDN
- [ ] 1.2 Declarar en `console/src/lib/` (p. ej. `lib/tour.ts`) la constante de versión del tour y el array declarativo de pasos `{ elemento, titulo, descripcion, rolMinimo }`, con selectores del shell y las 5 páginas (Ejecuciones, Puentes, Miembros, Credenciales, Bloques) y los pasos comunes (dashboard y trace) marcados para todos los roles
- [ ] 1.3 Implementar la función pura de filtrado de pasos por rol (reutilizando el rango de `lib/roles.ts`) y verificar con unitarias vitest: `viewer` sin pasos de Miembros ni de edición, `owner` con el recorrido completo y los pasos comunes presentes en todos los roles
- [ ] 1.4 Implementar el helper puro de `localStorage` (`leerTourVisto` / `marcarTourVisto`) con clave por workspace y valor de versión del tour, y verificar con unitarias vitest: primera lectura = no visto, tras marcar = visto, versión distinta = no visto, workspace distinto = no visto y almacenamiento que lanza = no visto sin excepción

## 2. Integración del tour en el shell

- [ ] 2.1 Montar la instancia de driver.js en `console/src/main.tsx` a partir de los pasos filtrados por el rol de la sesión, filtrando antes los pasos cuyo selector no exista en el DOM y sin lanzar el tour si no queda ningún paso (verificación: con `dev.sh`, un paso con selector inexistente se omite y el tour continúa)
- [ ] 2.2 Disparar el tour automáticamente al entrar a la consola solo cuando el helper indique que el navegador no lo ha visto para el workspace vigente, y marcarlo como visto al terminar (verificación: primera visita lo lanza solo; segunda visita no lo lanza)
- [ ] 2.3 Añadir el botón "Ver tutorial" al shell (escritorio junto al bloque de rol/salir del sidebar y móvil en la barra superior) reutilizando el estilo de botón fantasma sobre tinta, que relanza el tour desde el primer paso y funciona aunque ya esté marcado como visto (verificación: pulsarlo con el tour visto lo arranca desde el paso 1)
- [ ] 2.4 Destruir la instancia (`drv.destroy()`) al cerrar el tour, al navegar a otra ruta y al desmontar el shell (verificación: navegar con el tour abierto no deja popovers, resaltados ni manejadores de teclado activos)
- [ ] 2.5 Añadir las unitarias vitest que cubran la degradación segura y el filtrado por rol del punto 2.1 (selectores ausentes y roles), y dejar `npm test` verde

## 3. Presentación y accesibilidad del tour

- [ ] 3.1 Mapear el popover y el resaltado del tour a los tokens de El rastro (papel/superficie, tinta, bordes y tipografía de interfaz), verificando el nombre real de las variables en `node_modules/driver.js` y sin introducir colores de estado decorativos
- [ ] 3.2 Verificar contraste AA del popover y que no rompe `console/src/tema.test.ts` (`npm test` verde)
- [ ] 3.3 Respetar `prefers-reduced-motion` en el tour (sin transiciones no esenciales al cambiar de paso) y verificar manualmente con la preferencia activada en el navegador
- [ ] 3.4 Verificar el tour operable con teclado (avanzar, retroceder y cerrar) con foco visible en los controles del popover

## 4. Verificación integrada

- [ ] 4.1 Ejecutar `npm test` y `npm run build` en `console/` y verificar ambos verdes (incluye `tsc --noEmit`), comprobando que el bundle del tour queda dentro del tamaño aceptado (~20 KB gzip)
- [ ] 4.2 Verificar manualmente con `dev.sh` el flujo completo por rol: primera visita auto, relanzado por botón, ausencia en la segunda visita, respuesta de la consola sin salida a internet y ausencia de peticiones al backend originadas por el tour (DevTools, pestaña de red)
- [ ] 4.3 Actualizar `docs/guia-consola.md` con la sección del tour (qué es, lanzamiento automático la primera vez, botón "Ver tutorial", filtrado por rol y que el estado vive solo en el navegador) y verificar que el documento describe el comportamiento observable real

## Workflow follow-up

- Archivar este change cuando se cumplan los requisitos de revisión del proyecto.
- Actualizar `openspec/project.md` durante el archivado si procede (fuera del alcance de este change).
