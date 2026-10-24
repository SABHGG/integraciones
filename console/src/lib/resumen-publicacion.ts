//! Resumen de una publicación en una línea (confirmación de la acción
//! irreversible): qué trigger queda expuesto y cuántos bloques viajan.

/** Un bloque pendiente de publicar, con lo mínimo para el resumen. */
export interface BloqueResumenPublicacion {
  name: string;
}

/** Frase del resumen: `POST /x/demo · 3 bloques` según el trigger vigente. */
export function resumenPublicacion(
  trigger: unknown,
  bloques: BloqueResumenPublicacion[],
): string {
  let disparo = "disparador sin definir";
  if (typeof trigger === "object" && trigger !== null) {
    const t = trigger as Record<string, unknown>;
    const tipo = typeof t.type === "string" ? t.type : "";
    if (tipo === "http" && typeof t.method === "string" && typeof t.path === "string") {
      disparo = `${t.method} ${t.path}`;
    } else if (tipo === "cron" && typeof t.schedule === "string") {
      const zona = typeof t.timezone === "string" ? ` (${t.timezone})` : "";
      disparo = `cron ${t.schedule}${zona}`;
    } else if (tipo === "event" && typeof t.topic === "string") {
      disparo = `evento ${t.topic}`;
    } else if (tipo === "manual") {
      disparo = "manual";
    } else if (tipo !== "") {
      disparo = `disparador ${tipo}`;
    }
  }
  const n = bloques.length;
  return `${disparo} · ${n} ${n === 1 ? "bloque" : "bloques"}`;
}
