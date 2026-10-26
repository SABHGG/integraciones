//! Diff estructural de definiciones (MVP5, task 4.1, design D3): JSON
//! aplanado a rutas de hoja (`trigger.method`, `blocks.1.config.url`…),
//! comparado lado a lado, sin dependencias. Los arrays se comparan por
//! índice. Una sola fuente de verdad: el JSON congelado del servidor.

export interface LineaDiff {
  /** Ruta de la hoja donde difieren (p. ej. `blocks.0.name`). */
  ruta: string;
  /** Valor del lado izquierdo (AUSENTE si la ruta solo existe al otro lado). */
  izq: unknown;
  /** Valor del lado derecho (AUSENTE si la ruta solo existe al otro lado). */
  der: unknown;
}

/**
 * Aplana un JSON a rutas de hoja. Objetos recursan con notación por
 * puntos; arrays, por índice; primitivos y `null` son hojas. Los
 * contenedores vacíos son hojas en su propia ruta.
 */
export function aplanar(valor: unknown, prefijo = ""): Map<string, unknown> {
  const hojas = new Map<string, unknown>();
  if (Array.isArray(valor)) {
    if (valor.length === 0) {
      if (prefijo !== "") hojas.set(prefijo, []);
      return hojas;
    }
    valor.forEach((elemento, indice) => {
      for (const [ruta_hija, v] of aplanar(elemento, junta(prefijo, String(indice)))) {
        hojas.set(ruta_hija, v);
      }
    });
    return hojas;
  }
  if (valor !== null && typeof valor === "object") {
    const entradas = Object.entries(valor as Record<string, unknown>);
    if (entradas.length === 0) {
      if (prefijo !== "") hojas.set(prefijo, {});
      return hojas;
    }
    for (const [clave, v] of entradas) {
      for (const [ruta_hija, valor_hijo] of aplanar(v, junta(prefijo, clave))) {
        hojas.set(ruta_hija, valor_hijo);
      }
    }
    return hojas;
  }
  hojas.set(prefijo, valor);
  return hojas;
}

function junta(prefijo: string, clave: string): string {
  return prefijo === "" ? clave : `${prefijo}.${clave}`;
}

function igual(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** Marca visual de ausencia en el lado correspondiente. */
export const AUSENTE = "(ausente)";

/**
 * Diff lado a lado entre dos definiciones, ordenado por ruta. Las rutas
 * que solo existen en un lado muestran `AUSENTE` del otro.
 */
export function diffEstructural(izq: unknown, der: unknown): LineaDiff[] {
  const hojasIzq = aplanar(izq);
  const hojasDer = aplanar(der);
  const rutas = [...new Set([...hojasIzq.keys(), ...hojasDer.keys()])].sort();
  const lineas: LineaDiff[] = [];
  for (const ruta_actual of rutas) {
    const valorIzq = hojasIzq.get(ruta_actual);
    const valorDer = hojasDer.get(ruta_actual);
    if (!igual(valorIzq, valorDer)) {
      lineas.push({
        ruta: ruta_actual === "" ? "$" : ruta_actual,
        izq: hojasIzq.has(ruta_actual) ? valorIzq : AUSENTE,
        der: hojasDer.has(ruta_actual) ? valorDer : AUSENTE,
      });
    }
  }
  return lineas;
}

/** Resumen corto para la UI: «3 diferencias» / «idénticos». */
export function resumenDiff(lineas: LineaDiff[]): string {
  if (lineas.length === 0) return "idénticos";
  return `${lineas.length} diferencia${lineas.length === 1 ? "" : "s"}`;
}

/** Ruta de resolución del servidor que no es editable en el borrador: el
 código vive en la versión del bloque (el cambio de bloque ya se ve en
 `blockVersionId`); mostrar el JS congelado como «diferencia del puente»
 sería ruido de resolución, no información de decisión. */
const RUTA_CODE = /^blocks\.\d+\.code(?:\.|$)/;

/** Filtra líneas del diff a las secciones editables del borrador
 (`trigger`, `inputSchema`, `blocks`, `mask`, `emit`, `webhook`) y descarta
 las rutas de resolución `blocks.*.code…`. Función pura (D2 del design). */
export function lineasDeSecciones(lineas: LineaDiff[], secciones: readonly string[]): LineaDiff[] {
  const raices = new Set(secciones);
  return lineas.filter((linea) => {
    const raiz = linea.ruta.split(".")[0];
    if (!raices.has(raiz)) return false;
    return !(raiz === "blocks" && RUTA_CODE.test(linea.ruta));
  });
}
