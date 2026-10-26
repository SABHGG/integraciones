//! Tests de la validación ligera del trigger: casos espejo de `validate_trigger`
//! (crates/api/src/bridges.rs) y de `domain::cron`.
import { describe, expect, it } from "vitest";
import {
  clavesQuitadasAlCambiar,
  erroresDeFormulario,
  esTimezoneIana,
  validarCron,
  validarTopic,
  validarTrigger,
  zonasIana,
} from "./validacion-trigger";

const campos = (t: unknown) => validarTrigger(t).map((e) => e.campo);

describe("validarTopic", () => {
  it("acepta minúsculas, dígitos, puntos y guiones intermedios", () => {
    expect(validarTopic("pedidos.creado")).toBe(true);
    expect(validarTopic("a1.b2-c3")).toBe(true);
    expect(validarTopic("simple")).toBe(true);
  });
  it("rechaza separadores al inicio, final, consecutivos o mayúsculas", () => {
    expect(validarTopic(".pedidos")).toBe(false);
    expect(validarTopic("pedidos.")).toBe(false);
    expect(validarTopic("pedidos..creado")).toBe(false);
    expect(validarTopic("Pedidos")).toBe(false);
    expect(validarTopic("")).toBe(false);
  });
});

describe("validarCron", () => {
  it("acepta los crons de los talleres", () => {
    expect(validarCron("*/5 * * * *")).toBe(true);
    expect(validarCron("0 9 * * 1-5")).toBe(true);
    expect(validarCron("30 8 1,15 * 0")).toBe(true);
    expect(validarCron("7 8 */3 * *")).toBe(true);
  });
  it("rechaza 4 o 6 campos, rangos invertidos y fuera de rango", () => {
    expect(validarCron("* * * *")).toBe(false);
    expect(validarCron("* * * * * *")).toBe(false);
    expect(validarCron("60 * * * *")).toBe(false);
    expect(validarCron("5-1 * * * *")).toBe(false);
    expect(validarCron("* 25 * * *")).toBe(false);
    expect(validarCron("13 * * 0 *")).toBe(false);
    expect(validarCron("* * * * 7")).toBe(true); // 7 = domingo, el plano admite 0-7
    expect(validarCron("* * * * 8")).toBe(false);
    expect(validarCron("*/0 * * * *")).toBe(false);
    expect(validarCron("A * * * *")).toBe(false);
  });
});

describe("zonasIana / esTimezoneIana", () => {
  it("resuelve desde Intl y reconoce las zonas de los talleres", () => {
    expect(zonasIana().length).toBeGreaterThan(0);
    expect(esTimezoneIana("Europe/Madrid")).toBe(true);
    expect(esTimezoneIana("UTC")).toBe(true);
    expect(esTimezoneIana("Marte/Olympus")).toBe(false);
  });
});

describe("validarTrigger", () => {
  it("un trigger completo de cada tipo pasa limpio", () => {
    expect(validarTrigger({ type: "http", method: "POST", path: "/x/demo" })).toEqual([]);
    expect(validarTrigger({ type: "cron", schedule: "*/5 * * * *", timezone: "Europe/Madrid" })).toEqual([]);
    expect(validarTrigger({ type: "event", topic: "pedidos.creado" })).toEqual([]);
    expect(validarTrigger({ type: "manual" })).toEqual([]);
  });

  it("requiere type y rechaza tipos desconocidos", () => {
    expect(campos({})).toEqual(["type"]);
    expect(campos({ type: "rss" })).toEqual(["type"]);
  });

  it("http: method y path con los mismos mensajes de forma que el plano", () => {
    expect(campos({ type: "http" })).toEqual(["method", "path"]);
    expect(campos({ type: "http", method: "get", path: "x/demo" })).toEqual(["method", "path"]);
    expect(campos({ type: "http", method: "GET", path: "/x" })).toEqual([]);
  });

  it("cron: schedule y timezone invalidos se marcan por campo", () => {
    expect(campos({ type: "cron" })).toEqual(["schedule", "timezone"]);
    expect(campos({ type: "cron", schedule: "* * * *", timezone: "Marte/Olympus" })).toEqual([
      "schedule", "timezone",
    ]);
  });

  it("event: topic vacío o inválido se marca", () => {
    expect(campos({ type: "event" })).toEqual(["topic"]);
    expect(campos({ type: "event", topic: "Pedidos" })).toEqual(["topic"]);
  });

  it("no es un objeto: necesita disparador", () => {
    expect(campos(undefined)).toEqual(["type"]);
    expect(campos("http")).toEqual(["type"]);
  });
});

describe("clavesQuitadasAlCambiar", () => {
  it("http→cron pierde method y path; cron→http no pierde schedule si no tenía", () => {
    expect(clavesQuitadasAlCambiar("http", "cron")).toEqual(["method", "path"]);
    expect(clavesQuitadasAlCambiar("cron", "http")).toEqual(["schedule", "timezone"]);
  });
  it("manual pierde todas; subscribe al mismo tipo no quita nada", () => {
    expect(clavesQuitadasAlCambiar("http", "manual")).toEqual(["method", "path"]);
    expect(clavesQuitadasAlCambiar("http", "http")).toEqual([]);
  });
});

describe("erroresDeFormulario", () => {
  it("fusiona validación en cliente con issues del servidor (trigger.*)", () => {
    const mapa = erroresDeFormulario({ type: "http", method: "get", path: "x" }, [
      { field: "trigger.path", message: "el path debe empezar por /" },
      { field: "blocks", message: "la cadena necesita al menos un bloque" },
    ]);
    expect(mapa.method).toContain("no es un verbo HTTP");
    expect(mapa.path).toBe("el path debe empezar por /"); // el servidor manda
    expect(mapa.blocks).toBeUndefined(); // issues ajenos al trigger, fuera
  });

  it("trigger válido sin issues: mapa vacío", () => {
    expect(erroresDeFormulario({ type: "cron", schedule: "*/5 * * * *", timezone: "UTC" })).toEqual({});
  });
});
