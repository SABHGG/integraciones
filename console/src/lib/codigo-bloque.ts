//! Qué código mostrar por versión en el detalle de bloque y con qué etiqueta
//! (change add-block-source-code, design D5). Lógica pura, sin DOM, testeable
//! con vitest (`environment: "node"`).

/** Campos de una versión que deciden el visor de código. */
export interface VersionConCodigo {
  /** JS transpilado que ejecuta el sandbox (null si la versión no tiene código). */
  code: string | null;
  /** Fuente TypeScript original del autor (null si no existe). */
  sourceCode: string | null;
}

export interface CodigoVisible {
  /** Texto a pintar en el visor, o null cuando no hay nada que mostrar. */
  codigo: string | null;
  /**
   * Nota honesta sobre el código mostrado: `null` cuando es el fuente original
   * (lo que el autor escribió no necesita aclaración).
   */
  etiqueta: string | null;
}

export const ETIQUETA_JS_TRANSPILADO = "JS transpilado — versión anterior al change";
export const ETIQUETA_SIN_CODIGO = "built-in: sin código";

/**
 * Decide el texto y la etiqueta del visor de una versión:
 *
 * - con `sourceCode` → el fuente original, sin etiqueta;
 * - sin fuente y con `code` → el JS, etiquetado como transpilado de una versión
 *   anterior al change (nunca se presenta el JS emitido como si fuera el fuente);
 * - sin ninguno de los dos → sin código, dicho explícitamente.
 */
export function codigoAMostrar(version: VersionConCodigo): CodigoVisible {
  if (version.sourceCode != null && version.sourceCode !== "") {
    return { codigo: version.sourceCode, etiqueta: null };
  }
  if (version.code != null && version.code !== "") {
    return { codigo: version.code, etiqueta: ETIQUETA_JS_TRANSPILADO };
  }
  return { codigo: null, etiqueta: ETIQUETA_SIN_CODIGO };
}
