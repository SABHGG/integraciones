import { describe, it, expect, vi, afterEach } from "vitest";
import {
  rellenarSerie,
  formatearDuracion,
  picoSerie,
  crearPolling,
  INTERVALO_REFRESCO_MS,
} from "../src/lib/metricas";

afterEach(() => {
  vi.useRealTimers();
});

describe("rellenarSerie (task 5.1)", () => {
  it("rellena los días sin ejecuciones con ceros", () => {
    const serie = rellenarSerie("2026-10-18", "2026-10-21", [
      { day: "2026-10-18", total: 3, succeeded: 2, failed: 1 },
      { day: "2026-10-20", total: 5, succeeded: 5, failed: 0 },
    ]);
    expect(serie.map((d) => d.day)).toEqual([
      "2026-10-18",
      "2026-10-19",
      "2026-10-20",
      "2026-10-21",
    ]);
    expect(serie[1]).toEqual({ day: "2026-10-19", total: 0, succeeded: 0, failed: 0 });
    expect(serie[3].total).toBe(0);
  });

  it("un solo día da una serie de un elemento", () => {
    expect(rellenarSerie("2026-10-19", "2026-10-19", []).length).toBe(1);
  });

  it("conserva los datos de los días con ejecuciones", () => {
    const serie = rellenarSerie("2026-10-19", "2026-10-19", [
      { day: "2026-10-19", total: 7, succeeded: 6, failed: 1 },
    ]);
    expect(serie[0].total).toBe(7);
    expect(serie[0].succeeded).toBe(6);
  });
});

describe("formatearDuracion (task 5.1)", () => {
  it("ms por debajo del segundo, s con coma por encima", () => {
    expect(formatearDuracion(280)).toBe("280 ms");
    expect(formatearDuracion(1200)).toBe("1,2 s");
    expect(formatearDuracion(null)).toBe("—");
    expect(formatearDuracion(undefined)).toBe("—");
  });
});

describe("picoSerie", () => {
  it("mínimo 1 para no dividir por cero", () => {
    expect(picoSerie([{ day: "d", total: 0, succeeded: 0, failed: 0 }])).toBe(1);
    expect(picoSerie([{ day: "d", total: 9, succeeded: 0, failed: 0 }])).toBe(9);
  });
});

describe("crearPolling (task 5.3)", () => {
  it("el intervalo acordado es de 5 segundos", () => {
    expect(INTERVALO_REFRESCO_MS).toBe(5000);
  });

  it("ejecuta la tarea en cada tick y no deja peticiones solapadas", async () => {
    vi.useFakeTimers();
    let enVuelo = false;
    let llamadas = 0;
    const tarea = async () => {
      llamadas += 1;
      expect(enVuelo).toBe(false);
      enVuelo = true;
      await Promise.resolve();
      enVuelo = false;
    };
    const polling = crearPolling(5000, tarea, { estaOculto: () => false });
    polling.arrancar();
    await vi.advanceTimersByTimeAsync(15000);
    expect(llamadas).toBe(3);
    polling.parar();
    expect(polling.activo()).toBe(false);
  });

  it("se pausa con la pestaña oculta", async () => {
    vi.useFakeTimers();
    let llamadas = 0;
    const polling = crearPolling(5000, async () => {
      llamadas += 1;
    }, { estaOculto: () => true });
    polling.arrancar();
    await vi.advanceTimersByTimeAsync(15000);
    expect(llamadas).toBe(0);
    polling.parar();
  });

  it("arrancar dos veces no duplica el temporizador", async () => {
    vi.useFakeTimers();
    let llamadas = 0;
    const polling = crearPolling(5000, async () => {
      llamadas += 1;
    }, { estaOculto: () => false });
    polling.arrancar();
    polling.arrancar();
    await vi.advanceTimersByTimeAsync(5000);
    expect(llamadas).toBe(1);
    polling.parar();
  });
});
