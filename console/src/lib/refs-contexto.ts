//! Contexto de refs `$.` para el autocompletado del editor de puentes: dado el
//! tipo de disparador, el `inputSchema` del puente y las instancias previas de
//! la cadena (nombre + `output_schema` de su versión), produce las rutas que
//! el ejecutor podría resolver en ese punto. Espejo best-effort del contrato de
//! crates/core/src/refs.rs: raíces `$.trigger` y `$.now`, mapeo directo en
//! input e interpolación `{$.ref}` en strings de config. Un schema que no se
//! sabe expandir (allOf, if-then, primitivas sin type) aporta su raíz y basta:
//! el autocompletado es ayuda, la autoridad sigue en el servidor.

/** Una ruta disponible, completa y lista para interpolar (`$.a.b.0`). */
export interface SugerenciaRef {
  ruta: string;
  /** Raíz de procedencia: `trigger`, `now` o el nombre de la instancia previa. */
  fuente: string;
}

/** Entradas para calcular el contexto de refs en un punto de la cadena. */
export interface ContextoRefs {
  /** Tipo del disparador del borrador (`http|cron|event|manual`); ausente = sin trigger aún. */
  tipoTrigger?: string;
  /** JSON Schema del body del disparo (declara las propiedades de `$.trigger.body`). */
  inputSchema?: unknown;
  /** Instancias ANTERIORES en la cadena, en orden. */
  previas: { name: string; outputSchema: unknown }[];
}

/** Profundidad máxima de expansión de un schema (protección contra ciclos). */
const PROFUNDIDAD_MAXIMA = 6;

/** Raíces de `$.trigger` según el tipo congelado (definition.rs TriggerType). */
const RAICES_TRIGGER: Record<string, string[]> = {
  http: ["$.trigger.body", "$.trigger.query"],
  cron: ["$.trigger.scheduledAt", "$.trigger.cronExpr", "$.trigger.timezone"],
  event: ["$.trigger.topic", "$.trigger.payload", "$.trigger.receivedAt"],
};

function esObjeto(valor: unknown): valor is Record<string, unknown> {
  return typeof valor === "object" && valor !== null && !Array.isArray(valor);
}

/** Aplana un JSON Schema en rutas `$.`: `properties` recursivo, `array.items`
 con índice de ejemplo `0`, y la ruta raíz siempre incluida. */
function expandirSchema(
  schema: unknown,
  prefijo: string,
  fuente: string,
  profundidad: number,
  salida: Map<string, SugerenciaRef>,
): void {
  if (profundidad > PROFUNDIDAD_MAXIMA) return;
  // La raíz ya puede estar (p. ej. `$.trigger.body` viene de las raíces fijas):
  // no se re-registra, pero las propiedades SÍ se expanden.
  if (!salida.has(prefijo)) salida.set(prefijo, { ruta: prefijo, fuente });
  if (!esObjeto(schema)) return;

  if (esObjeto(schema.properties)) {
    for (const [clave, subSchema] of Object.entries(schema.properties)) {
      expandirSchema(subSchema, `${prefijo}.${clave}`, fuente, profundidad + 1, salida);
    }
    return;
  }
  // Array: la ruta al array y un primer elemento de ejemplo (`items.0`), que
  // es la forma en que refs.rs indexa (índices numéricos).
  if (schema.items !== undefined) {
    expandirSchema(schema.items, `${prefijo}.0`, fuente, profundidad + 1, salida);
  }
}

/** Rutas de `$.trigger…` según el tipo: raíces fijas + propiedades del
 `inputSchema` cuando aplican (http → body; manual → la raíz misma). */
function rutasTrigger(tipo: string | undefined, inputSchema: unknown, salida: Map<string, SugerenciaRef>): void {
  const raices = (tipo && RAICES_TRIGGER[tipo]) ?? ["$.trigger"];
  for (const raiz of raices) salida.set(raiz, { ruta: raiz, fuente: "trigger" });

  if (!esObjeto(inputSchema) || !esObjeto(inputSchema.properties)) return;
  if (tipo === "http") {
    expandirSchema(inputSchema, "$.trigger.body", "trigger", 1, salida);
  } else if (tipo === "manual" || tipo === undefined) {
    expandirSchema({ properties: inputSchema.properties }, "$.trigger", "trigger", 1, salida);
  }
  // event/cron: el payload es libre (datos de prueba tal cual), solo raíces fijas.
}

/** Calcula las sugerencias de refs disponibles en un punto de la cadena. */
export function sugerenciasDeRefs(contexto: ContextoRefs): SugerenciaRef[] {
  const salida = new Map<string, SugerenciaRef>();
  rutasTrigger(contexto.tipoTrigger, contexto.inputSchema, salida);
  salida.set("$.now", { ruta: "$.now", fuente: "now" });

  for (const previa of contexto.previas) {
    if (!previa.name.trim()) continue;
    expandirSchema(previa.outputSchema, `$.${previa.name}`, previa.name, 1, salida);
  }
  return [...salida.values()];
}

/** Inserta una sugerencia en un texto en la posición del cursor: interpolación
 `{$.ref}` (strings de config) o mapeo directo `$.ref` (input). Devuelve el
 texto nuevo y la posición del cursor tras la inserción. */
export function insertarSugerencia(
  texto: string,
  cursor: number,
  ruta: string,
  modo: "interpolar" | "mapear",
): { texto: string; cursor: number } {
  const pieza = modo === "interpolar" ? `{${ruta}}` : ruta;
  const antes = texto.slice(0, cursor);
  const despues = texto.slice(cursor);
  // Modo mapeo: si entre el último `$.` y el cursor solo hay fragmento de ruta
  // (letras, dígitos, punto o guion bajo), la ref a medio escribir se SUSTITUYE
  // en vez de concatenarse detrás.
  const inicioRef = modo === "mapear" ? antes.lastIndexOf("$.") : -1;
  if (inicioRef >= 0 && /^[\w.]*$/.test(antes.slice(inicioRef + 2))) {
    const nuevo = antes.slice(0, inicioRef) + pieza + despues;
    return { texto: nuevo, cursor: inicioRef + pieza.length };
  }
  const nuevo = antes + pieza + despues;
  return { texto: nuevo, cursor: cursor + pieza.length };
}
