//! Tests del selector de bloques del editor (G3): filtrado + agrupación.
import { describe, expect, it } from "vitest";
import { bloquesAgrupados, type BloqueResumen } from "./bloques";

const b = (id: string, slug: string, name: string, blockType: string): BloqueResumen => ({
  id, slug, name, blockType, category: "", description: "", latestVersion: 1,
});

const biblioteca = [
  b("1", "http-get", "Llamada GET", "http"),
  b("2", "enrich-order", "Enriquecer pedido", "custom_logic"),
  b("3", "http-post", "Llamada POST", "http"),
  b("4", "validar-rfc", "Validar RFC", "custom_logic"),
];

describe("bloquesAgrupados", () => {
  it("sin texto: todos, agrupados por blockType en orden de aparición", () => {
    const grupos = bloquesAgrupados(biblioteca, "");
    expect(grupos.map((g) => g.tipo)).toEqual(["http", "custom_logic"]);
    expect(grupos[0].bloques.map((x) => x.slug)).toEqual(["http-get", "http-post"]);
  });
  it("filtra por slug y nombre sin mayúsculas", () => {
    expect(bloquesAgrupados(biblioteca, "HTTP-P").flatMap((g) => g.bloques.map((x) => x.slug))).toEqual(["http-post"]);
    expect(bloquesAgrupados(biblioteca, "pedido").flatMap((g) => g.bloques.map((x) => x.slug))).toEqual(["enrich-order"]);
  });
  it("sin coincidencias: lista vacía", () => {
    expect(bloquesAgrupados(biblioteca, "zzz")).toEqual([]);
  });
});
