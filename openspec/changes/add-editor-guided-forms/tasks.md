# Tasks

## 1. Módulos puros en lib/ (base, con tests)

- [x] 1.1 Crear `console/src/lib/refs-contexto.ts`: expansión best-effort de rutas `$.` (raíces `$.now`/`$.trigger` según tipo, `properties` recursivo, `array.items` con índice `0`, profundidad acotada) y tests vitest que cubren http con inputSchema, event/cron, bloques previos y schemas no expandibles (verificación: `npm run test` en verde).
- [x] 1.2 Crear `console/src/lib/validacion-trigger.ts`: validación ligera en cliente (5 verbos, path `/`, topic regex, cron de 5 campos con rangos, timezone con `Intl.supportedValuesOf('timeZone')` + fallback) y tests con casos espejo de `validate_trigger` (verificación: `npm run test` en verde; el formulario nunca bloquea algo que el servidor acepta).
- [x] 1.3 Crear `console/src/lib/schema-builder.ts`: parseo de `inputSchema` a campos simples + resto conservado, y fusión de vuelta sin descartar restricciones finas; tests de ida y vuelta con `pattern`/`additionalProperties` (verificación: `npm run test` en verde).
- [x] 1.4 Ampliar `console/src/lib/ui-forms.ts` a los 7 widgets del contrato (`json`, `select`, `ref-template` añadidos) y exponer las opciones `enum` del `configSchema` por campo; tests de `camposDeUi` con los nuevos widgets y de caída a `text` para ids desconocidos (verificación: `npm run test` en verde).

## 2. Autocompletado de refs

- [x] 2.1 Crear componente `console/src/ui/input-con-refs.tsx`: dropdown anclado al cursor (botón `$` + Ctrl/Cmd+Espacio) sobre textareas e inputs, con inserción en `selectionStart` — `{$.ruta}` en config, `$.ruta` en input; tests de inserción y de foco (verificación: `npm run test` en verde).
- [ ] 2.2 Cablear `InputConRefs` en las textarea de `input` y `config` (sin `ui`) del editor de puentes y en el widget `ref-template` de `FormularioInstancia`, alimentado por `refs-contexto.ts` con las instancias previas reales; comprobación manual en `npm run dev` con el puente del taller rick-and-morty (verificación: las sugerencias muestran `$.trigger…`, `$.now` y la salida del bloque 1, y la inserción produce interpolación/mapeo correctos).

## 3. Formulario guiado del trigger

- [x] 3.1 Crear `FormularioTrigger` en `console/src/paginas/bridges.tsx`: selector de tipo y campos por tipo (dropdown de 5 verbos + path; schedule + timezone con datalist IANA; topic; manual sin campos) usando `validacion-trigger.ts` para errores en línea; test de componente/lógica de estado trigger↔formulario (verificación: `npm run test` en verde).
- [x] 3.2 Añadir la pestaña «JSON avanzado» compartiendo estado con el formulario guiado (ida y vuelta sin pérdida) y sustituir la llamada actual a `FormularioInstancia ui={null}` del panel Trigger; comprobación manual del escenario de ida y vuelta del spec (verificación: `npm run build` sin errores y flujo visible en `npm run dev`).

## 4. Constructor visual del inputSchema

- [x] 4.1 Crear el constructor de campos (nombre, tipo string|number|boolean|object|array, requerido) sobre `schema-builder.ts` con pestaña JSON avanzada de ida y vuelta, y sustituir la textarea del panel Input schema; tests del merge conservador ya en 1.3 + comprobación manual de que un `pattern` pegado sobrevive al guardar (verificación: `npm run build` y flujo visible en `npm run dev`).

## 5. Widgets completos en formularios generados

- [x] 5.1 Extender `console/src/ui/blockform.tsx`: widget `select` (opciones del `enum` del `configSchema`, cae a texto sin él), `json` (editor JSON con validación de sintaxis y valor estructurado) y `ref-template` (`InputConRefs` con interpolación); tests de render lógico por widget (verificación: `npm run test` en verde).

## 6. Autoría de metadatos ui al publicar versión

- [x] 6.1 Añadir en `console/src/paginas/blocks.tsx` la subsección de metadatos `ui` del formulario de publicación (title, order ordenable, widget por campo con desplegable de los 7 ids, help por campo), que envía `ui` en `POST /blocks/{id}/versions` solo cuando se declaró algo; comprobación manual publicando un bloque con `ui` y viendo el formulario guiado en un puente nuevo (verificación: el 200 de publicación incluye ui y la instancia del bloque muestra el formulario generado).
- [x] 6.2 Mantener la publicación sin `ui` byte a byte igual que hoy (sin claves nuevas en el body) y verificar con el flujo existente del taller de credenciales (verificación: `npm run build` y publicación de un bloque custom_logic sin tocar ui sigue funcionando).

## 7. Integración y cierre

- [ ] 7.1 Suite completa y build de producción: `npm run test` y `npm run build` (tsc --noEmit + vite) en verde; repaso de que ningún requisito del delta de `specs/web-console/spec.md` queda sin escenario manual comprobado (verificación: comandos en verde + lista de escenarios recorrida).
