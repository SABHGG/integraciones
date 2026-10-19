//! Lógica pura del dashboard (MVP5, tasks 5.1 y 5.3): serie diaria con
//! huecos rellenados, formato de duración y el temporizador de
//! auto-refresh (5 s, apagado por defecto, pausa en pestaña oculta y sin
//! peticiones solapadas). Sin DOM: testeable con Vitest.

export interface DiaCrudo {
  day: string;
  total: number;
  succeeded: number;
  failed: number;
}

export interface DiaCompleto {
  day: string;
  total: number;
  succeeded: number;
  failed: number;
}

/**
 * Rellena los días sin ejecuciones con ceros entre `desde` y `hasta`
 * (YYYY-MM-DD inclusive). La API solo devuelve días con datos.
 */
export function rellenarSerie(
  desde: string,
  hasta: string,
  daily: DiaCrudo[],
): DiaCompleto[] {
  const porDia = new Map(daily.map((dia) => [dia.day, dia]));
  const serie: DiaCompleto[] = [];
  const actual = new Date(`${desde}T00:00:00Z`);
  const fin = new Date(`${hasta}T00:00:00Z`);
  while (actual <= fin) {
    const clave = actual.toISOString().slice(0, 10);
    const crudo = porDia.get(clave);
    serie.push({
      day: clave,
      total: crudo?.total ?? 0,
      succeeded: crudo?.succeeded ?? 0,
      failed: crudo?.failed ?? 0,
    });
    actual.setUTCDate(actual.getUTCDate() + 1);
  }
  return serie;
}

/** Duración legible: «—» si no hay, ms por debajo del segundo, si no, s. */
export function formatearDuracion(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || Number.isNaN(ms)) return "—";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(1).replace(".", ",")} s`;
}

/** Pico de la serie para escalar las barras (mínimo 1 para evitar ÷0). */
export function picoSerie(serie: DiaCompleto[]): number {
  return Math.max(1, ...serie.map((dia) => dia.total));
}

export interface Polling {
  arrancar: () => void;
  parar: () => void;
  activo: () => boolean;
}

/**
 * Auto-refresh (task 5.3): cada `intervaloMs` ejecuta `tarea` SI la pestaña
 * está visible y SI la tarea anterior terminó (sin peticiones solapadas).
 */
export function crearPolling(
  intervaloMs: number,
  tarea: () => Promise<void>,
  opciones: { estaOculto: () => boolean },
): Polling {
  let temporizador: ReturnType<typeof setInterval> | null = null;
  let enVuelo = false;

  const tic = () => {
    if (opciones.estaOculto() || enVuelo) return;
    enVuelo = true;
    void tarea().finally(() => {
      enVuelo = false;
    });
  };

  return {
    arrancar: () => {
      if (temporizador !== null) return;
      temporizador = setInterval(tic, intervaloMs);
    },
    parar: () => {
      if (temporizador !== null) {
        clearInterval(temporizador);
        temporizador = null;
      }
    },
    activo: () => temporizador !== null,
  };
}

export const INTERVALO_REFRESCO_MS = 5000;
