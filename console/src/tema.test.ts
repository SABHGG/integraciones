//! Verificación de contraste WCAG de los tokens del tema (task 1.4): cada par
//! cromo que usa la interfaz debe cumplir AA (4.5:1 en texto normal). Los
//! tokens viven en tema.css; este test los lee de ahí (node:fs) para que no
//! puedan divergir del CSS real.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const raiz = join(dirname(fileURLToPath(import.meta.url)));
const css = readFileSync(join(raiz, "tema.css"), "utf8");

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

/** Mezcla un color sobre otro con opacidad (para fondos X/5, X/8…). */
function mezclar(frente: string, fondo: string, alfa: number): string {
  const c = (h: string, i: number) => parseInt(h.slice(1 + i, 3 + i), 16);
  const ch = (i: number) =>
    Math.round(c(frente, i) * alfa + c(fondo, i) * (1 - alfa));
  return `#${[0, 2, 4].map((i) => ch(i).toString(16).padStart(2, "0")).join("")}`;
}

function contraste(a: string, b: string): number {
  const [l1, l2] = [luminancia(a), luminancia(b)].sort((x, y) => y - x);
  return (l1 + 0.05) / (l2 + 0.05);
}

const AA = 4.5;

describe("contraste WCAG del sistema El rastro", () => {
  it("texto principal sobre papel y superficie", () => {
    expect(contraste(token("tinta"), token("papel"))).toBeGreaterThanOrEqual(AA);
    expect(contraste(token("tinta"), token("superficie"))).toBeGreaterThanOrEqual(AA);
  });

  it("sidebar: papel sobre tinta (texto grande y normal)", () => {
    expect(contraste(token("papel"), token("tinta"))).toBeGreaterThanOrEqual(AA);
  });

  it("texto secundario sobre superficie y papel", () => {
    expect(contraste(token("tinta-suave"), token("superficie"))).toBeGreaterThanOrEqual(AA);
    expect(contraste(token("tinta-suave"), token("papel"))).toBeGreaterThanOrEqual(AA);
  });

  it("estados sobre sus fondos de insignia", () => {
    for (const [texto, fondo] of [
      ["ok", "ok-fondo"],
      ["mal", "mal-fondo"],
      ["aviso", "aviso-fondo"],
      ["info", "info-fondo"],
    ] as const) {
      expect(contraste(token(texto), token(fondo)), `${texto} sobre ${fondo}`).toBeGreaterThanOrEqual(AA);
    }
  });

  it("error en línea sobre su fondo tenue (mal/5 sobre superficie)", () => {
    const fondo = mezclar(token("mal"), token("superficie"), 0.05);
    expect(contraste(token("mal"), fondo)).toBeGreaterThanOrEqual(AA);
  });

  it("placeholder y metadato terciario sobre superficie", () => {
    // texto pequeño de apoyo: también AA (no se acepta menor por ser "decorativo")
    expect(contraste(token("tinta-tenue"), token("superficie"))).toBeGreaterThanOrEqual(AA);
  });
});
