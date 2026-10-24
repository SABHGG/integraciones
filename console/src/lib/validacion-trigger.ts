//! Validación ligera en cliente del disparador, espejo de `validate_trigger`
//! (crates/api/src/bridges.rs) y de lo esencial de `domain::cron`: solo lo
//! barato de calcular en el navegador para avisar en línea. El servidor sigue
//! siendo la única autoridad: nada aquí bloquea lo que el servidor aceptaría
//! (ante la duda, se deja pasar y el 422 manda).

export const VERBOS_HTTP = ["GET", "POST", "PUT", "PATCH", "DELETE"] as const;
export const TIPOS_TRIGGER = ["http", "cron", "event", "manual"] as const;

/** Error de un campo del trigger; `campo` es la clave dentro del trigger
 (`method`, `schedule`, …), igual que el sufijo de `trigger.<campo>` del plano. */
export interface ErrorTrigger {
  campo: string;
  mensaje: string;
}

// ── topic (definition.rs validar_topic) ──────────────────────────────────────

/** Topic: partes separadas por `.` o `-`, cada una no vacía y solo [a-z0-9]. */
export function validarTopic(topic: string): boolean {
  if (topic === "") return false;
  return topic.split(/[.-]/).every((parte) => {
    if (parte === "") return false;
    return [...parte].every((c) => (c >= "a" && c <= "z") || (c >= "0" && c <= "9"));
  });
}

// ── cron (espejo ligero de domain::cron::parse) ──────────────────────────────

const RANGOS_CRON: [number, number][] = [
  [0, 59], // minuto
  [0, 23], // hora
  [1, 31], // día del mes
  [1, 12], // mes
  [0, 7], // día de la semana (7 = domingo, lo normaliza el servidor)
];

/** Cron de 5 campos: `*`, número, rango ascendente y paso, con listas por
 coma; rangos por posición como el plano. Sin nombres de mes/día (el plano
 tampoco los acepta). */
export function validarCron(expr: string): boolean {
  const campos = expr.trim().split(/\s+/);
  if (campos.length !== 5) return false;
  return campos.every((campo, indice) => {
    const [min, max] = RANGOS_CRON[indice];
    return campo.split(",").every((item) => {
      const [rango, paso] = item.split("/");
      if (paso !== undefined && (!/^\d+$/.test(paso) || Number(paso) < 1)) return false;
      if (rango === "*") return true;
      const m = /^(\d+)(-(\d+))?$/.exec(rango);
      if (!m) return false;
      const desde = Number(m[1]);
      if (desde < min || desde > max) return false;
      if (m[3] === undefined) return true;
      const hasta = Number(m[3]);
      return hasta >= desde && hasta <= max;
    });
  });
}

// ── timezone (cron::validate_timezone) ───────────────────────────────────────

/** Lista mínima para navegadores sin `Intl.supportedValuesOf`; la validación
 real sigue en servidor (`chrono_tz`). */
const ZONAS_FALLBACK = [
  "UTC", "Europe/Madrid", "Europe/Lisbon", "Europe/London", "Europe/Paris",
  "America/New_York", "America/Chicago", "America/Mexico_City", "America/Bogota",
  "America/Lima", "America/Argentina/Buenos_Aires", "America/Sao_Paulo",
  "Asia/Tokyo", "Asia/Shanghai", "Asia/Kolkata", "Australia/Sydney",
];

let cacheZonas: string[] | null = null;

/** Zonas IANA del runtime (`Intl.supportedValuesOf("timeZone")`) con fallback. */
export function zonasIana(): string[] {
  if (cacheZonas) return cacheZonas;
  try {
    const intl = Intl as unknown as { supportedValuesOf?: (clave: string) => string[] };
    const zonas = intl.supportedValuesOf?.("timeZone");
    if (Array.isArray(zonas) && zonas.length > 0) {
      cacheZonas = zonas;
      return zonas;
    }
  } catch {
    // Navegador viejo: caemos a la lista mínima.
  }
  cacheZonas = ZONAS_FALLBACK;
  return cacheZonas;
}

export function esTimezoneIana(zona: string): boolean {
  // chrono_tz acepta "UTC" aunque no venga en supportedValuesOf.
  return zona === "UTC" || zonasIana().includes(zona);
}

// ── validación del trigger completo ──────────────────────────────────────────

/** Errores de forma de un trigger de borrador (los mismos que reportará el
 servidor al validar o publicar, con la clave del campo como localizador). */
export function validarTrigger(trigger: unknown): ErrorTrigger[] {
  if (typeof trigger !== "object" || trigger === null || Array.isArray(trigger)) {
    return [{ campo: "type", mensaje: "el puente necesita un disparador" }];
  }
  const t = trigger as Record<string, unknown>;
  const tipo = t.type;
  if (typeof tipo !== "string" || tipo === "") {
    return [{ campo: "type", mensaje: 'el disparador necesita "type" (http|cron|event|manual)' }];
  }
  const errores: ErrorTrigger[] = [];

  if (tipo === "http") {
    if (typeof t.method !== "string" || t.method === "") {
      errores.push({ campo: "method", mensaje: 'un disparador http necesita "method"' });
    } else if (!(VERBOS_HTTP as readonly string[]).includes(t.method)) {
      errores.push({
        campo: "method",
        mensaje: `"${t.method}" no es un verbo HTTP soportado (usa mayúsculas: GET, POST, PUT, PATCH, DELETE)`,
      });
    }
    if (typeof t.path !== "string" || t.path === "") {
      errores.push({ campo: "path", mensaje: 'un disparador http necesita "path" (la ruta pública del puente)' });
    } else if (!t.path.startsWith("/")) {
      errores.push({ campo: "path", mensaje: `el path "${t.path}" debe empezar por "/"` });
    }
  } else if (tipo === "cron") {
    if (typeof t.schedule !== "string" || t.schedule === "") {
      errores.push({ campo: "schedule", mensaje: 'un disparador cron necesita "schedule" (expresión cron de 5 campos)' });
    } else if (!validarCron(t.schedule)) {
      errores.push({ campo: "schedule", mensaje: `"${t.schedule}" no es un cron de 5 campos válido` });
    }
    if (typeof t.timezone !== "string" || t.timezone === "") {
      errores.push({ campo: "timezone", mensaje: 'un disparador cron necesita "timezone" (zona IANA, p. ej. Europe/Madrid)' });
    } else if (!esTimezoneIana(t.timezone)) {
      errores.push({ campo: "timezone", mensaje: `"${t.timezone}" no es una zona horaria IANA conocida` });
    }
  } else if (tipo === "event") {
    if (typeof t.topic !== "string" || t.topic === "") {
      errores.push({ campo: "topic", mensaje: 'un disparador event necesita "topic" (el canal al que se suscribe el puente)' });
    } else if (!validarTopic(t.topic)) {
      errores.push({
        campo: "topic",
        mensaje: `"${t.topic}" no es un topic válido (minúsculas, dígitos, puntos y guiones; sin separadores al inicio, al final ni consecutivos)`,
      });
    }
  } else if (tipo !== "manual") {
    errores.push({
      campo: "type",
      mensaje: `"${tipo}" no es un tipo de disparador soportado (http|cron|event|manual)`,
    });
  }
  return errores;
}

/** Mapa campo → mensaje para el formulario guiado: fusiona la validación en
 cliente con los issues del servidor (cuyo field viene como `trigger.method`).
 La pestaña JSON avanzada lista los mismos errores sin localizar campo. */
export function erroresDeFormulario(
  trigger: unknown,
  issuesServidor?: { field: string; message: string }[],
): Record<string, string> {
  const mapa: Record<string, string> = {};
  for (const e of validarTrigger(trigger)) mapa[e.campo] = e.mensaje;
  for (const issue of issuesServidor ?? []) {
    if (issue.field.startsWith("trigger.")) mapa[issue.field.slice("trigger.".length)] = issue.message;
  }
  return mapa;
}
