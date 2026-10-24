//! Tests del contexto de refs `$.` (sugerencias + inserción en cursor).
import { describe, expect, it } from "vitest";
import { insertarSugerencia, sugerenciasDeRefs } from "./refs-contexto";

const rutas = (s: ReturnType<typeof sugerenciasDeRefs>) => s.map((x) => x.ruta);

describe("sugerenciasDeRefs", () => {
  it("sin nada: raíces base ($.trigger y $.now)", () => {
    const s = sugerenciasDeRefs({ previas: [] });
    expect(rutas(s)).toEqual(["$.trigger", "$.now"]);
  });

  it("http: raíces body/query + propiedades del inputSchema bajo body", () => {
    const s = sugerenciasDeRefs({
      tipoTrigger: "http",
      inputSchema: {
        type: "object",
        properties: { userId: { type: "string" }, limite: { type: "number" } },
        required: ["userId"],
      },
      previas: [],
    });
    expect(rutas(s)).toEqual([
      "$.trigger.body", "$.trigger.query",
      "$.trigger.body.userId", "$.trigger.body.limite",
      "$.now",
    ]);
  });

  it("cron y event: raíces fijas de su forma congelada", () => {
    expect(rutas(sugerenciasDeRefs({ tipoTrigger: "cron", previas: [] }))).toEqual([
      "$.trigger.scheduledAt", "$.trigger.cronExpr", "$.trigger.timezone", "$.now",
    ]);
    expect(rutas(sugerenciasDeRefs({ tipoTrigger: "event", previas: [] }))).toEqual([
      "$.trigger.topic", "$.trigger.payload", "$.trigger.receivedAt", "$.now",
    ]);
  });

  it("manual: propiedades del inputSchema directamente bajo $.trigger", () => {
    const s = sugerenciasDeRefs({
      tipoTrigger: "manual",
      inputSchema: { properties: { cantidad: { type: "number" } } },
      previas: [],
    });
    expect(rutas(s)).toEqual(["$.trigger", "$.trigger.cantidad", "$.now"]);
  });

  it("expande salidas de bloques previos: objeto, array con índice 0 y anidado", () => {
    const s = sugerenciasDeRefs({
      tipoTrigger: "http",
      previas: [
        {
          name: "usuario",
          outputSchema: {
            properties: {
              id: { type: "string" },
              tags: { items: { type: "string" } },
              meta: { properties: { total: { type: "number" } } },
            },
          },
        },
      ],
    });
    expect(rutas(s)).toContain("$.usuario.id");
    expect(rutas(s)).toContain("$.usuario.tags");
    expect(rutas(s)).toContain("$.usuario.tags.0");
    expect(rutas(s)).toContain("$.usuario.meta.total");
    // La fuente es el nombre de la instancia.
    expect(s.find((x) => x.ruta === "$.usuario.id")?.fuente).toBe("usuario");
  });

  it("profundidad acotada: no explota con schemas infinitamente anidados", () => {
    let profundo: Record<string, unknown> = { type: "string" };
    for (let i = 0; i < 40; i++) profundo = { properties: { hijo: profundo } };
    const s = sugerenciasDeRefs({
      tipoTrigger: "manual",
      previas: [{ name: "x", outputSchema: profundo }],
    });
    const deX = s.filter((x) => x.ruta.startsWith("$.x."));
    expect(deX.length).toBeLessThanOrEqual(8);
    expect(rutas(s)).toContain("$.trigger");
  });

  it("previa sin nombre o con schema vacío aporta al menos su raíz", () => {
    const s = sugerenciasDeRefs({
      tipoTrigger: "manual",
      previas: [{ name: "b1", outputSchema: null }, { name: "  ", outputSchema: { type: "object" } }],
    });
    expect(rutas(s)).toContain("$.b1");
    expect(rutas(s)).not.toContain("$.  ");
  });
});

describe("insertarSugerencia", () => {
  it("interpola {$...} en config conservando el texto previo", () => {
    const r = insertarSugerencia("https://api/x", 13, "$.trigger.body.userId", "interpolar");
    expect(r.texto).toBe("https://api/x{$.trigger.body.userId}");
    expect(r.cursor).toBe(13 + "$.trigger.body.userId".length + 2);
  });

  it("mapea $.ref directo en input", () => {
    const r = insertarSugerencia('{"userId": ', 11, "$.usuario.id", "mapear");
    expect(r.texto).toBe('{"userId": $.usuario.id');
    expect(r.cursor).toBe(11 + "$.usuario.id".length);
  });

  it("mapeo sustituye un $. a medio escribir en lugar de concatenar", () => {
    const r = insertarSugerencia("$.us", 4, "$.usuario.id", "mapear");
    expect(r.texto).toBe("$.usuario.id");
    expect(r.cursor).toBe("$.usuario.id".length);
  });

  it("mapeo también sustituye si tras el fragmento viene una comilla", () => {
    const r = insertarSugerencia('{"a": $.x"b"}', 9, "$.y", "mapear");
    expect(r.texto).toBe('{"a": $.y"b"}');
  });

  it("mapeo no sustituye si entre $. y el cursor hay espacio", () => {
    const r = insertarSugerencia('{"a": $.b "x"', 13, "$.y", "mapear");
    expect(r.texto).toBe('{"a": $.b "x"$.y');
  });
});
