import { describe, it, expect } from "vitest";
import { aplanar, diffEstructural, resumenDiff, AUSENTE } from "../src/lib/diff";

describe("aplanar", () => {
  it("aplana objetos anidados con notación por puntos", () => {
    const hojas = aplanar({ trigger: { method: "POST", path: "/x" } });
    expect(hojas.get("trigger.method")).toBe("POST");
    expect(hojas.get("trigger.path")).toBe("/x");
  });

  it("aplana arrays por índice", () => {
    const hojas = aplanar({ blocks: [{ name: "a" }, { name: "b" }] });
    expect(hojas.get("blocks.0.name")).toBe("a");
    expect(hojas.get("blocks.1.name")).toBe("b");
  });

  it("null es hoja; contenedores vacíos también", () => {
    const hojas = aplanar({ n: null, vacio: [], obj: {} });
    expect(hojas.get("n")).toBeNull();
    expect(hojas.get("vacio")).toEqual([]);
    expect(hojas.get("obj")).toEqual({});
  });
});

describe("diffEstructural", () => {
  const v1 = {
    trigger: { type: "http", method: "POST" },
    blocks: [{ id: "b1", name: "eco", config: { url: "/a" } }],
  };
  const v2 = {
    trigger: { type: "http", method: "PUT" },
    blocks: [{ id: "b1", name: "eco", config: { url: "/b" } }],
    mask: ["$.trigger.body.tarjeta"],
  };

  it("detecta solo las hojas que cambian", () => {
    const lineas = diffEstructural(v1, v2);
    const rutas = lineas.map((l) => l.ruta);
    expect(rutas).toContain("trigger.method");
    expect(rutas).toContain("blocks.0.config.url");
    // Los arrays se comparan por índice: una regla mask nueva es mask.0.
    expect(rutas).toContain("mask.0");
    expect(lineas.find((l) => l.ruta === "trigger.method")?.izq).toBe("POST");
    expect(lineas.find((l) => l.ruta === "trigger.method")?.der).toBe("PUT");
  });

  it("las rutas nuevas muestran AUSENTE en el lado izquierdo", () => {
    const lineas = diffEstructural(v1, v2);
    const mask = lineas.find((l) => l.ruta === "mask.0");
    expect(mask?.izq).toBe(AUSENTE);
    expect(mask?.der).toBe("$.trigger.body.tarjeta");
  });

  it("idénticos → sin líneas", () => {
    expect(diffEstructural(v1, structuredClone(v1))).toEqual([]);
  });

  it("borrador contra versión funciona en ambas direcciones", () => {
    expect(diffEstructural(v2, v1).length).toBe(diffEstructural(v1, v2).length);
  });
});

describe("resumenDiff", () => {
  it("resume el conteo o la igualdad", () => {
    expect(resumenDiff([])).toBe("idénticos");
    expect(resumenDiff([{ ruta: "a", izq: 1, der: 2 }])).toBe("1 diferencia");
    expect(resumenDiff([{ ruta: "a", izq: 1, der: 2 }, { ruta: "b", izq: 1, der: 2 }])).toBe(
      "2 diferencias",
    );
  });
});
