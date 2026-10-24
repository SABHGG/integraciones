//! Parseo JSON tolerante para los editores texto-a-JSON de la consola: un
//! texto vacío cuenta como "sin valor" (no como error) y un texto inválido
//! NO se aplica — pero se reporta, para que nunca vuelva a haber un catch
//! silencioso dejando el borrador obsoleto sin avisar (critique P0-2).

export type ParseoJson =
  | { ok: true; valor: undefined }
  | { ok: true; valor: unknown }
  | { ok: false; error: string };

/** Parsea texto JSON: `""`/blancos → ok con `undefined` (campo vacío); JSON
 válido → su valor; inválido → error con mensaje legible. */
export function parseSeguro(texto: string): ParseoJson {
  if (texto.trim() === "") return { ok: true, valor: undefined };
  try {
    return { ok: true, valor: JSON.parse(texto) };
  } catch (e) {
    const mensaje = e instanceof Error ? e.message : "JSON inválido";
    return { ok: false, error: `JSON inválido: ${mensaje}` };
  }
}

/** Serializa un valor para el textarea: undefined → "" (vacío), el resto →
 JSON con indentación 2. */
export function textoDe(valor: unknown): string {
  return valor === undefined ? "" : JSON.stringify(valor, null, 2);
}
