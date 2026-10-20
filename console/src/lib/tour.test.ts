//! Unitarias del tour de onboarding (tasks 1.3, 1.4, 2.5 y 3.2): el entorno de
//! vitest es `node`, sin DOM, así que aquí se prueban las reglas puras —
//! filtrado por rol, degradación segura ante selectores ausentes, helper de
//! almacenamiento y contraste AA de los pares que usa el popover.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { Rol } from "./roles";
import {
  CLAVE_TOUR,
  PASOS_TOUR,
  VERSION_TOUR,
  claveTour,
  leerTourVisto,
  marcarTourVisto,
  movimientoReducido,
  pasosDisponibles,
  type Almacen,
} from "./tour";

/** Almacenamiento de mentira con la forma de `localStorage` (stub Map). */
function almacenStub(inicial: Record<string, string> = {}): Almacen & { datos: Map<string, string> } {
  const datos = new Map(Object.entries(inicial));
  return {
    datos,
    getItem: (clave) => datos.get(clave) ?? null,
    setItem: (clave, valor) => {
      datos.set(clave, valor);
    },
  };
}

const TODOS: Rol[] = ["viewer", "runner", "editor", "owner"];
const titulos = (rol: Rol) => pasosDisponibles(rol).map((p) => p.titulo);

describe("declaración de pasos (task 1.2)", () => {
  it("versión vigente declarada como entero positivo", () => {
    expect(Number.isInteger(VERSION_TOUR)).toBe(true);
    expect(VERSION_TOUR).toBeGreaterThan(0);
  });

  it("cada paso tiene selector, textos y rol válidos", () => {
    expect(PASOS_TOUR.length).toBeGreaterThan(0);
    for (const paso of PASOS_TOUR) {
      expect(paso.elemento.trim()).not.toBe("");
      expect(paso.titulo.trim()).not.toBe("");
      expect(paso.descripcion.trim()).not.toBe("");
      expect(TODOS).toContain(paso.rolMinimo);
    }
  });

  it("cubre el shell y las cinco secciones de la consola", () => {
    const selectores = PASOS_TOUR.map((p) => p.elemento).join(" ");
    for (const ruta of ["executions", "bridges", "members", "credentials", "blocks"]) {
      expect(selectores).toContain(`/console/${ruta}`);
    }
    expect(selectores).toContain('aside nav[aria-label="Secciones"]');
    expect(selectores).toContain('header select[aria-label="Sección"]');
  });

  it("no referencia ninguna CDN (la consola es offline-first)", () => {
    const texto = PASOS_TOUR.map((p) => `${p.titulo} ${p.descripcion} ${p.elemento}`).join(" ");
    expect(texto).not.toMatch(/https?:\/\//);
    expect(texto).not.toMatch(/cdn/i);
  });
});

describe("filtrado de pasos por rol (task 1.3)", () => {
  it("viewer no ve gestión de miembros ni edición", () => {
    const pasos = pasosDisponibles("viewer");
    expect(pasos.length).toBeGreaterThan(0);
    expect(pasos.every((p) => p.rolMinimo === "viewer")).toBe(true);
    expect(pasos.map((p) => p.elemento).join(" ")).not.toContain("/console/members");
    expect(titulos("viewer")).not.toContain("Editar, validar y publicar");
    expect(titulos("viewer")).not.toContain("Crear y rotar credenciales");
  });

  it("owner recorre el tour completo", () => {
    expect(pasosDisponibles("owner")).toHaveLength(PASOS_TOUR.length);
  });

  it("el recorrido crece con el rol, sin perder pasos", () => {
    const viewer = pasosDisponibles("viewer");
    const runner = pasosDisponibles("runner");
    const editor = pasosDisponibles("editor");
    const owner = pasosDisponibles("owner");
    expect(runner.length).toBeGreaterThanOrEqual(viewer.length);
    expect(editor.length).toBeGreaterThan(runner.length);
    expect(owner.length).toBeGreaterThan(editor.length);
    // los pasos de un rol inferior son subconjunto de los del superior
    for (const paso of viewer) expect(owner).toContain(paso);
  });

  it("los pasos comunes (dashboard y trace) están en todos los roles", () => {
    const comunes = PASOS_TOUR.filter((p) => p.rolMinimo === "viewer").map((p) => p.titulo);
    expect(comunes).toContain("El rastro en miniatura");
    expect(comunes).toContain("El trace paso a paso");
    for (const rol of TODOS) {
      for (const titulo of ["El rastro en miniatura", "El trace paso a paso"]) {
        expect(titulos(rol)).toContain(titulo);
      }
    }
  });

  it("sin rol no hay pasos (no se inventa recorrido)", () => {
    expect(pasosDisponibles(null)).toEqual([]);
    expect(pasosDisponibles(undefined)).toEqual([]);
  });
});

describe("degradación segura ante selectores ausentes (task 2.5)", () => {
  it("descarta los pasos cuyo selector no existe en el DOM", () => {
    const pasos = pasosDisponibles("owner", (selector) => selector.includes("/console/"));
    expect(pasos.length).toBeGreaterThan(0);
    expect(pasos.every((p) => p.elemento.includes("/console/"))).toBe(true);
  });

  it("si no queda ningún paso devuelve vacío (el tour no se lanza)", () => {
    expect(pasosDisponibles("owner", () => false)).toEqual([]);
  });

  it("el filtro por selector no altera el orden declarado", () => {
    const pasos = pasosDisponibles("owner", (selector) => selector.includes("nav"));
    const esperado = PASOS_TOUR.filter((p) => p.elemento.includes("nav")).map((p) => p.titulo);
    expect(pasos.map((p) => p.titulo)).toEqual(esperado);
  });
});

describe("helper de localStorage del tour (task 1.4)", () => {
  it("primera lectura en un navegador limpio = no visto", () => {
    const almacen = almacenStub();
    expect(leerTourVisto("ws-1", almacen)).toBe(false);
    expect(almacen.datos.size).toBe(0); // leer no escribe
  });

  it("tras marcar = visto", () => {
    const almacen = almacenStub();
    marcarTourVisto("ws-1", almacen);
    expect(leerTourVisto("ws-1", almacen)).toBe(true);
  });

  it("la clave incluye el workspace y el valor la versión", () => {
    const almacen = almacenStub();
    marcarTourVisto("ws-1", almacen);
    expect(almacen.datos.get(claveTour("ws-1"))).toBe(String(VERSION_TOUR));
    expect(claveTour("ws-1")).toBe(`${CLAVE_TOUR}.ws-1`);
    expect(claveTour("ws-1")).not.toBe(claveTour("ws-2"));
  });

  it("versión distinta de la vigente = no visto", () => {
    const almacen = almacenStub({ [claveTour("ws-1")]: String(VERSION_TOUR + 1) });
    expect(leerTourVisto("ws-1", almacen)).toBe(false);
  });

  it("valor corrupto = no visto", () => {
    const almacen = almacenStub({ [claveTour("ws-1")]: "sí, lo vi" });
    expect(leerTourVisto("ws-1", almacen)).toBe(false);
  });

  it("workspace distinto = no visto", () => {
    const almacen = almacenStub();
    marcarTourVisto("ws-1", almacen);
    expect(leerTourVisto("ws-2", almacen)).toBe(false);
  });

  it("sin workspace usa una clave estable propia", () => {
    const almacen = almacenStub();
    marcarTourVisto(null, almacen);
    expect(leerTourVisto(null, almacen)).toBe(true);
    expect(leerTourVisto(undefined, almacen)).toBe(true);
    expect(leerTourVisto("ws-1", almacen)).toBe(false);
    expect(claveTour(null)).toBe(`${CLAVE_TOUR}.sin-workspace`);
  });

  it("almacenamiento que lanza al leer = no visto, sin excepción", () => {
    const roto: Almacen = {
      getItem: () => {
        throw new Error("SecurityError: modo privado");
      },
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
    };
    expect(() => leerTourVisto("ws-1", roto)).not.toThrow();
    expect(leerTourVisto("ws-1", roto)).toBe(false);
  });

  it("almacenamiento que lanza al escribir no propaga", () => {
    const roto: Almacen = {
      getItem: () => null,
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
    };
    expect(() => marcarTourVisto("ws-1", roto)).not.toThrow();
  });

  it("sin almacenamiento disponible = no visto, sin excepción", () => {
    expect(leerTourVisto("ws-1", null)).toBe(false);
    expect(() => marcarTourVisto("ws-1", null)).not.toThrow();
  });
});

describe("movimiento reducido", () => {
  it("en node (sin matchMedia) degrada a movimiento normal", () => {
    expect(movimientoReducido()).toBe(false);
  });
});

// ── Contraste AA del popover (task 3.2) ──
// El popover del tour solo usa pares ya validados por tema.test.ts; se
// re-verifican aquí porque el mapeo vive en tema.css y debe seguir cumpliendo
// AA si alguien toca los tokens.
const raiz = dirname(fileURLToPath(import.meta.url));
const css = readFileSync(join(raiz, "..", "tema.css"), "utf8");

function token(nombre: string): string {
  const m = css.match(new RegExp(`--color-${nombre}:\\s*(#[0-9a-fA-F]{6})`));
  if (!m) throw new Error(`token --color-${nombre} no encontrado en tema.css`);
  return m[1].toLowerCase();
}

function luminancia(hex: string): number {
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(hex.slice(1 + i, 3 + i), 16) / 255);
  const lin = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function contraste(a: string, b: string): number {
  const [l1, l2] = [luminancia(a), luminancia(b)].sort((x, y) => y - x);
  return (l1 + 0.05) / (l2 + 0.05);
}

describe("contraste AA del popover del tour (task 3.2)", () => {
  it("título y descripción sobre la superficie del popover", () => {
    expect(contraste(token("tinta"), token("superficie"))).toBeGreaterThanOrEqual(4.5);
    expect(contraste(token("tinta-suave"), token("superficie"))).toBeGreaterThanOrEqual(4.5);
    expect(contraste(token("tinta-tenue"), token("superficie"))).toBeGreaterThanOrEqual(4.5);
  });

  it("el popover mapea tokens de El rastro y no introduce colores de estado", () => {
    const bloque = css.slice(css.indexOf(".driver-popover"));
    expect(bloque.length).toBeGreaterThan(0);
    expect(bloque).toContain("var(--color-superficie)");
    expect(bloque).toContain("var(--color-tinta)");
    expect(bloque).toContain("var(--font-sans)");
    for (const estado of ["--color-ok", "--color-mal", "--color-aviso", "--color-info"]) {
      expect(bloque).not.toContain(estado);
    }
  });

  it("respeta prefers-reduced-motion en el tour", () => {
    expect(css).toMatch(/prefers-reduced-motion[^}]*\{[\s\S]*--driver-animation-duration/);
  });
});
