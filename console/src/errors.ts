//! Mapeo de errores del validador del servidor sobre el canvas/editor.
//! La consola NO valida por su cuenta (una sola fuente de verdad): pinta los
//! 422 del servidor donde corresponda.

export interface Issue {
  field: string;
  message: string;
}

export interface ErroresMapeados {
  /** Issues que no caen en una instancia concreta (trigger, mask, …). */
  globales: { field: string; message: string }[];
  /** Issues por índice de instancia en `blocks` (del draft). */
  porInstancia: Map<number, { field: string; message: string }[]>;
}

const PATRON_INSTANCIA = /^blocks\.(\d+)/;

/// Reparte los issues del servidor entre globales y por instancia.
export function mapearErrores(issues: Issue[]): ErroresMapeados {
  const mapeado: ErroresMapeados = { globales: [], porInstancia: new Map() };
  for (const issue of issues) {
    const coincidencia = PATRON_INSTANCIA.exec(issue.field);
    if (coincidencia) {
      const indice = Number(coincidencia[1]);
      const lista = mapeado.porInstancia.get(indice) ?? [];
      lista.push(issue);
      mapeado.porInstancia.set(indice, lista);
    } else {
      mapeado.globales.push(issue);
    }
  }
  return mapeado;
}

/// Extrae la línea de un mensaje de transpilación
/// (`… (línea 3, columna 7): …`) para anotar el editor de código.
export function lineaDeTranspilacion(mensaje: string): number | null {
  const coincidencia = /línea (\d+)/.exec(mensaje);
  return coincidencia ? Number(coincidencia[1]) : null;
}
