import { describe, it, expect } from "vitest";
import {
  validarCreacion,
  validarRotacion,
  confirmarEliminacion,
  esVigente,
} from "../src/lib/credenciales";

describe("validarCreacion", () => {
  it("acepta un formulario completo", () => {
    expect(validarCreacion({ name: "stripe-key", secret: "sk_123" })).toEqual([]);
  });

  it("exige nombre y secreto", () => {
    const issues = validarCreacion({ name: "  ", secret: "" });
    expect(issues).toContainEqual({ field: "name", message: "el nombre es obligatorio" });
    expect(issues).toContainEqual({ field: "secret", message: "el secreto es obligatorio" });
  });

  it("el nombre se recorta antes de decidir", () => {
    expect(validarCreacion({ name: "  k  ", secret: "v" })).toEqual([]);
  });
});

describe("validarRotacion", () => {
  it("exige el valor nuevo (nunca pre-llenado)", () => {
    expect(validarRotacion("")).toEqual([
      { field: "secret", message: "introduce el valor nuevo para rotar" },
    ]);
    expect(validarRotacion("nuevo-valor")).toEqual([]);
  });
});

describe("confirmarEliminacion", () => {
  it("exige teclear el nombre exacto", () => {
    expect(confirmarEliminacion("stripe", "stripe")).toBe(true);
    expect(confirmarEliminacion("stripe", "  stripe ")).toBe(true);
    expect(confirmarEliminacion("stripe", "otra")).toBe(false);
    expect(confirmarEliminacion("stripe", "")).toBe(false);
  });

  it("nunca confirma un nombre vacío", () => {
    expect(confirmarEliminacion("", "")).toBe(false);
  });
});

describe("esVigente", () => {
  it("marca la versión vigente del historial", () => {
    expect(esVigente("v-2", "v-2")).toBe(true);
    expect(esVigente("v-1", "v-2")).toBe(false);
    expect(esVigente("v-1", null)).toBe(false);
  });
});
