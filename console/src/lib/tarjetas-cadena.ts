//! Lógica de las tarjetas de la cadena (critique P1): qué tarjetas empiezan
//! abiertas (solo la última; las que tienen errores se fuerzan abiertas) y
//! quitar/reinsertar con Deshacer — la instancia quitada se conserva en
//! memoria de sesión hasta que el usuario decida.

/** Posiciones abiertas tras un cambio de longitud: se conservan las válidas
 y la última instancia nueva queda abierta (dedo en el rastro). */
export function purgarAbiertas(prev: readonly number[], total: number): number[] {
  const limpias = prev.filter((i) => i >= 0 && i < total);
  if (total > 0) limpias.push(total - 1);
  return [...new Set(limpias)].sort((a, b) => a - b);
}

/** Quita la instancia en `indice`: devuelve las que quedan y la quitada
 (para el Deshacer). Índice fuera de rango: no altera nada. */
export function quitarInstancia<T>(
  lista: readonly T[],
  indice: number,
): { quedan: T[]; quitada: T | undefined } {
  if (indice < 0 || indice >= lista.length) return { quedan: [...lista], quitada: undefined };
  const quedan = [...lista];
  const [quitada] = quedan.splice(indice, 1);
  return { quedan, quitada };
}

/** Reinserta la instancia en su índice original (acotado) al deshacer. */
export function reinsertarInstancia<T>(lista: readonly T[], indice: number, instancia: T): T[] {
  const copia = [...lista];
  copia.splice(Math.max(0, Math.min(indice, copia.length)), 0, instancia);
  return copia;
}

/** ¿Está expandida la tarjeta? Con errores siempre (el error necesita sitio);
 sin ellos, solo si el usuario la abrió. */
export function estaExpandida(indice: number, abiertas: readonly number[], conErrores: boolean): boolean {
  return conErrores || abiertas.includes(indice);
}
