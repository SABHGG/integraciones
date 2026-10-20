//! Componentes base del sistema visual "El rastro": botones, insignias de
//! estado, paneles y campos. Toda la identidad viaja por aquí; las páginas
//! no inventan estilos propios.
import type { ComponentChildren } from "preact";
import { h } from "preact";

/** Botón: primario = tinta sólida (la acción de la página), secundario = borde,
 peligro = ladrillo (el color sí significa: destruye o publica irreversible). */
export function Boton(props: {
  variante?: "primario" | "secundario" | "peligro";
  chico?: boolean;
  children: ComponentChildren;
  procesando?: boolean;
  class?: string;
  type?: "button" | "submit";
  deshabilitado?: boolean;
  onClick?: () => void;
}) {
  const base =
    "inline-flex items-center justify-center gap-1.5 rounded-[7px] font-medium transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-tinta disabled:opacity-50 disabled:cursor-default";
  const tamano = props.chico ? "px-2.5 py-1.5 text-[12.5px]" : "px-3.5 py-2 text-[13.5px]";
  const variantes = {
    primario: "bg-tinta text-papel hover:bg-[#0E2822] disabled:hover:bg-tinta",
    secundario:
      "border border-borde-fuerte bg-superficie text-tinta hover:border-tinta-suave",
    peligro: "bg-mal text-superficie hover:bg-[#93301F]",
  } as const;
  return (
    <button
      type={props.type ?? "button"}
      disabled={props.deshabilitado}
      class={`${base} ${tamano} ${variantes[props.variante ?? "primario"]} ${props.class ?? ""}`}
      onClick={props.onClick}
    >
      {props.children}
    </button>
  );
}

/** Estados de ejecución y de paso: el único cromo del sistema. */
export type Estado = "succeeded" | "failed" | "running" | "pending" | "skipped";

const ESTADOS: Record<Estado, { texto: string; fondo: string }> = {
  succeeded: { texto: "text-ok", fondo: "bg-ok-fondo" },
  failed: { texto: "text-mal", fondo: "bg-mal-fondo" },
  running: { texto: "text-info", fondo: "bg-info-fondo" },
  pending: { texto: "text-aviso", fondo: "bg-aviso-fondo" },
  skipped: { texto: "text-aviso", fondo: "bg-aviso-fondo" },
};

/** Insignia de estado: valor de máquina en mono con LED del color de estado. */
export function Insignia(props: { estado: string; class?: string }) {
  const e = ESTADOS[props.estado as Estado] ?? ESTADOS.pending;
  return (
    <span
      class={`inline-flex items-center gap-1.5 rounded-[4px] px-2 py-1 font-mono text-[11px] font-medium ${e.texto} ${e.fondo} ${props.class ?? ""}`}
    >
      <span
        class={`h-1.5 w-1.5 rounded-full bg-current ${props.estado === "running" ? "animate-pulse motion-reduce:animate-none" : ""}`}
      />
      {props.estado}
    </span>
  );
}

/** Panel: superficie con borde; la única elevación del sistema. */
export function Panel(props: { children: ComponentChildren; class?: string }) {
  return (
    <section
      class={`rounded-panel border border-borde bg-superficie ${props.class ?? ""}`}
    >
      {props.children}
    </section>
  );
}

/** Campo de formulario: etiqueta visible siempre, ayuda y error en línea. */
export function Campo(props: {
  etiqueta: string;
  children: ComponentChildren;
  ayuda?: string;
  error?: string;
  class?: string;
}) {
  return (
    <label class={`block ${props.class ?? ""}`}>
      <span class="mb-1.5 block text-[13px] font-medium text-tinta">{props.etiqueta}</span>
      {props.children}
      {props.ayuda && !props.error && (
        <span class="mt-1.5 block text-[12px] text-tinta-tenue">{props.ayuda}</span>
      )}
      {props.error && (
        <span class="mt-1.5 block rounded-[6px] border border-mal/40 bg-mal/5 px-2 py-1 text-[12.5px] text-mal">
          {props.error}
        </span>
      )}
    </label>
  );
}

/** Control de formulario común (input/select/textarea). */
export function Control(props: {
  as?: "input" | "select" | "textarea";
  mono?: boolean;
  class?: string;
  children?: ComponentChildren;
  [extra: string]: unknown;
}) {
  const { as = "input", mono, class: clase, children, ...resto } = props;
  return h(
    as,
    {
      ...resto,
      class: `w-full rounded-[7px] border border-borde-fuerte bg-superficie px-2.5 py-2 text-[13.5px] text-tinta placeholder:text-tinta-tenue focus:border-tinta-suave focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-tinta ${mono ? "font-mono text-[12.5px]" : ""} ${clase ?? ""}`,
    },
    children,
  );
}

/** Esqueleto de carga: neutro, sin spinner. */
export function Esqueleto(props: { filas?: number; class?: string }) {
  const filas = props.filas ?? 4;
  return (
    <div class={`space-y-2.5 p-4 ${props.class ?? ""}`} aria-hidden="true">
      {Array.from({ length: filas }, (_, i) => (
        <div
          key={i}
          class="h-9 rounded-[6px] bg-tinta-tenue/20"
          style={`width: ${94 - ((i * 13) % 31)}%`}
        />
      ))}
    </div>
  );
}
