import { describe, it, expect } from "vitest";
import { puede, etiquetaRol, type Rol } from "../src/lib/roles";

describe("puede (espejo de la matriz del servidor)", () => {
  it("viewer solo lee", () => {
    const rol: Rol = "viewer";
    expect(puede(rol, "leer")).toBe(true);
    expect(puede(rol, "ejecutar")).toBe(false);
    expect(puede(rol, "administrar-recursos")).toBe(false);
    expect(puede(rol, "gestionar-miembros")).toBe(false);
  });

  it("runner ejecuta pero no administra", () => {
    const rol: Rol = "runner";
    expect(puede(rol, "ejecutar")).toBe(true);
    expect(puede(rol, "administrar-recursos")).toBe(false);
    expect(puede(rol, "gestionar-miembros")).toBe(false);
  });

  it("editor administra recursos pero no gestiona miembros", () => {
    const rol: Rol = "editor";
    expect(puede(rol, "ejecutar")).toBe(true);
    expect(puede(rol, "administrar-recursos")).toBe(true);
    expect(puede(rol, "gestionar-miembros")).toBe(false);
  });

  it("owner puede todo", () => {
    const rol: Rol = "owner";
    expect(puede(rol, "leer")).toBe(true);
    expect(puede(rol, "ejecutar")).toBe(true);
    expect(puede(rol, "administrar-recursos")).toBe(true);
    expect(puede(rol, "gestionar-miembros")).toBe(true);
  });

  it("sin rol no puede nada", () => {
    expect(puede(null, "leer")).toBe(false);
    expect(puede(undefined, "leer")).toBe(false);
  });
});

describe("etiquetaRol", () => {
  it("etiqueta roles conocidos y el fallback", () => {
    expect(etiquetaRol("owner")).toBe("owner");
    expect(etiquetaRol(null)).toBe("sin rol");
  });
});
