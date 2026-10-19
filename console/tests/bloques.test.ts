import { describe, it, expect } from "vitest";
import {
  filtrarBloques,
  categorias,
  validarCreacionBloque,
  erroresDelPlano,
  type BloqueResumen,
} from "../src/bloques";

const bloques: BloqueResumen[] = [
  {
    id: "1",
    slug: "http-get",
    name: "HTTP GET",
    blockType: "http",
    category: "red",
    description: "Petición GET",
    latestVersion: 3,
  },
  {
    id: "2",
    slug: "enrich-order",
    name: "Enriquecer pedido",
    blockType: "custom_logic",
    category: "dominio",
    description: "Añade datos al pedido",
    latestVersion: 1,
  },
  {
    id: "3",
    slug: "http-post",
    name: "HTTP POST",
    blockType: "http",
    category: "red",
    description: "Petición POST",
    latestVersion: null,
  },
];

describe("filtrarBloques", () => {
  it("filtra por texto libre sin distinguir mayúsculas", () => {
    const r = filtrarBloques(bloques, { texto: "ENRIQUE" });
    expect(r.map((b) => b.slug)).toEqual(["enrich-order"]);
  });

  it("filtra por tipo y categoría combinados", () => {
    expect(filtrarBloques(bloques, { tipo: "http" }).map((b) => b.slug)).toEqual([
      "http-get",
      "http-post",
    ]);
    expect(
      filtrarBloques(bloques, { tipo: "http", categoria: "dominio" }),
    ).toEqual([]);
  });

  it("busca también en la descripción", () => {
    expect(filtrarBloques(bloques, { texto: "datos" }).map((b) => b.slug)).toEqual([
      "enrich-order",
    ]);
  });
});

describe("categorias", () => {
  it("devuelve categorías distintas y ordenadas", () => {
    expect(categorias(bloques)).toEqual(["dominio", "red"]);
  });
});

describe("validarCreacionBloque", () => {
  it("acepta un slug kebab-case válido", () => {
    expect(validarCreacionBloque({ slug: "enrich-order", name: "X" })).toEqual([]);
  });

  it("rechaza slug vacío, con mayúsculas o espacios, y nombre vacío", () => {
    expect(validarCreacionBloque({ slug: "", name: "" }).map((i) => i.field)).toEqual([
      "slug",
      "name",
    ]);
    expect(
      validarCreacionBloque({ slug: "Enrich Order", name: "X" }).map((i) => i.field),
    ).toEqual(["slug"]);
  });
});

describe("erroresDelPlano", () => {
  it("combina detail e issues en mensajes para el editor", () => {
    expect(
      erroresDelPlano("la publicación falló", [
        { field: "code", message: "línea 3: variable no definida" },
      ]),
    ).toEqual(["la publicación falló", "code: línea 3: variable no definida"]);
  });

  it("omite el detail vacío", () => {
    expect(erroresDelPlano("", [{ field: "code", message: "x" }])).toEqual([
      "code: x",
    ]);
  });
});
