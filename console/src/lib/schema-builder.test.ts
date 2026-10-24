//! Tests del constructor de inputSchema: merge conservador (D5) — nada de
//! `pattern`/`additionalProperties` se pierde por pasar por el formulario.
import { describe, expect, it } from "vitest";
import { desglosarSchema, montarSchema } from "./schema-builder";

describe("desglosarSchema", () => {
  it("separa campos simples editables del resto", () => {
    const d = desglosarSchema({
      type: "object",
      properties: { userId: { type: "string" }, limite: { type: "number" } },
      required: ["userId"],
      additionalProperties: false,
    });
    expect(d.campos).toEqual([
      { nombre: "userId", tipo: "string", requerido: true },
      { nombre: "limite", tipo: "number", requerido: false },
    ]);
    expect(d.resto).toEqual({ type: "object", additionalProperties: false });
  });

  it("una propiedad con restricciones finas NO es editable y se conserva entera", () => {
    const d = desglosarSchema({
      properties: {
        email: { type: "string", pattern: "^\\S+@\\S+$" },
        simple: { type: "string" },
      },
    });
    expect(d.campos).toEqual([{ nombre: "simple", tipo: "string", requerido: false }]);
    expect(d.resto.properties).toEqual({ email: { type: "string", pattern: "^\\S+@\\S+$" } });
  });

  it("required de propiedades no editables se conserva en el resto", () => {
    const d = desglosarSchema({
      properties: { email: { type: "string", format: "email" } },
      required: ["email"],
    });
    expect(d.campos).toEqual([]);
    expect(d.resto.required).toEqual(["email"]);
  });

  it("schema no-objeto: campos vacíos y resto vacío", () => {
    expect(desglosarSchema(null)).toEqual({ campos: [], resto: {} });
    expect(desglosarSchema("x")).toEqual({ campos: [], resto: {} });
  });
});

describe("montarSchema", () => {
  it("fusiona campos sobre el resto sin tocarlo", () => {
    const s = montarSchema(
      [
        { nombre: "userId", tipo: "string", requerido: true },
        { nombre: "limite", tipo: "number", requerido: false },
      ],
      { type: "object", additionalProperties: false },
    );
    expect(s).toEqual({
      type: "object",
      additionalProperties: false,
      properties: { userId: { type: "string" }, limite: { type: "number" } },
      required: ["userId"],
    });
  });

  it("ida y vuelta con pattern intacto", () => {
    const original = {
      type: "object",
      properties: { email: { type: "string", pattern: "^\\S+@\\S+$" }, n: { type: "number" } },
      required: ["email"],
    };
    const d = desglosarSchema(original);
    const montado = montarSchema(d.campos, d.resto);
    expect(montado).toEqual(original);
  });

  it("sin campos ni required en el resto: no emite properties/required", () => {
    const s = montarSchema([], {});
    expect(s).toEqual({});
  });

  it("desmarcar un required edita required sin duplicar entradas del resto", () => {
    const d = desglosarSchema({
      properties: { a: { type: "string" }, b: { type: "string" } },
      required: ["a", "b"],
    });
    d.campos.find((c) => c.nombre === "a")!.requerido = false;
    const s = montarSchema(d.campos, d.resto);
    expect(s.required).toEqual(["b"]);
  });
});
