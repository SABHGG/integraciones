//! Campo de texto/textarea con autocompletado de refs `$.` (design D3): un
//! dropdown anclado al control existente — botón `$` + Ctrl/Cmd+Espacio — que
//! inserta en la posición del cursor: interpolación `{$.ref}` en strings de
//! config o mapeo directo `$.ref` en el input de la instancia. La lógica de
//! inserción y filtrado vive en funciones puras exportadas (testeables sin
//! DOM); el servidor sigue siendo la autoridad de si la ref resuelve.
import { useEffect, useId, useState } from "preact/hooks";
import { Control } from "./ui";
import type { SugerenciaRef } from "../lib/refs-contexto";
import { insertarSugerencia } from "../lib/refs-contexto";

/** Fragmento de ruta a medio escribir justo antes del cursor: la cola de
 caracteres de ruta (`[\w$.]`) que sirve para filtrar sugerencias. */
export function fragmentoEnCursor(texto: string, cursor: number): string {
  const antes = texto.slice(0, cursor);
  const m = /[\w$.]*$/.exec(antes);
  return m ? m[0] : "";
}

/** Sugerencias filtradas por el fragmento (substring, sin distinguir mayús.)
 y acotadas a una lista manejable. */
export function filtrarSugerencias(
  sugerencias: SugerenciaRef[],
  fragmento: string,
  maximo = 8,
): { visibles: SugerenciaRef[]; ocultas: number } {
  const aguja = fragmento.toLowerCase();
  const encajan = aguja === ""
    ? sugerencias
    : sugerencias.filter((s) => s.ruta.toLowerCase().includes(aguja));
  return { visibles: encajan.slice(0, maximo), ocultas: Math.max(0, encajan.length - maximo) };
}

export function InputConRefs(props: {
  valor: string;
  onInput: (nuevo: string) => void;
  sugerencias: SugerenciaRef[];
  /** `interpolar` inserta `{$.ref}` (strings de config); `mapear` inserta `$.ref` (input). */
  modo: "interpolar" | "mapear";
  multilinea?: boolean;
  filas?: number;
  id?: string;
  placeholder?: string;
  ariaLabel?: string;
}) {
  const [abierto, setAbierto] = useState(false);
  const [activa, setActiva] = useState(0);
  const [filtro, setFiltro] = useState("");
  const idListbox = useId();
  // El nodo real (input/textarea): guarda el control para leer/mover el cursor.
  const [control, setControl] = useState<HTMLInputElement | HTMLTextAreaElement | null>(null);
  // Texto crudo interno: la edición a medio parsear NO se revierte (P0-2);
  // un cambio externo del valor (rollback, recarga) sí se adopta.
  const [crudo, setCrudo] = useState(() => props.valor);
  const [reportado, setReportado] = useState(() => props.valor);

  useEffect(() => {
    if (props.valor !== reportado) {
      setReportado(props.valor);
      setCrudo(props.valor);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.valor]);

  const { visibles, ocultas } = filtrarSugerencias(props.sugerencias, filtro);

  const abrir = () => {
    if (!control || props.sugerencias.length === 0) return;
    setFiltro(fragmentoEnCursor(crudo, control.selectionStart ?? crudo.length));
    setActiva(0);
    setAbierto(true);
  };

  const elegir = (ruta: string) => {
    if (!control) return;
    const cursor = control.selectionStart ?? crudo.length;
    const r = insertarSugerencia(crudo, cursor, ruta, props.modo);
    // El re-render controlado fijará el mismo texto; el cursor se conserva.
    control.value = r.texto;
    control.setSelectionRange(r.cursor, r.cursor);
    setCrudo(r.texto);
    setReportado(r.texto);
    props.onInput(r.texto);
    setAbierto(false);
    control.focus();
  };

  const enTecla = (e: KeyboardEvent) => {
    const nodo = e.target as HTMLInputElement | HTMLTextAreaElement;
    if ((e.ctrlKey || e.metaKey) && e.code === "Space") {
      e.preventDefault();
      abrir();
    } else if (abierto && e.key === "Escape") {
      setAbierto(false);
    } else if (abierto && visibles.length > 0 && (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Enter")) {
      e.preventDefault();
      if (e.key === "ArrowDown") setActiva((a) => Math.min(a + 1, visibles.length - 1));
      else if (e.key === "ArrowUp") setActiva((a) => Math.max(a - 1, 0));
      else elegir(visibles[activa].ruta);
    } else if (abierto) {
      setFiltro(fragmentoEnCursor(nodo.value, nodo.selectionStart ?? 0));
    }
  };

  const alEscribir = (e: Event) => {
    const nodo = e.target as HTMLInputElement | HTMLTextAreaElement;
    setCrudo(nodo.value);
    setReportado(nodo.value);
    props.onInput(nodo.value);
    if (abierto) setFiltro(fragmentoEnCursor(nodo.value, nodo.selectionStart ?? 0));
  };

  return (
    <div class="relative">
      <Control
        as={props.multilinea ? "textarea" : "input"}
        type={props.multilinea ? undefined : "text"}
        id={props.id}
        mono={props.multilinea}
        rows={props.multilinea ? props.filas ?? 3 : undefined}
        aria-label={props.ariaLabel}
        placeholder={props.placeholder}
        ref={setControl}
        value={crudo}
        onInput={alEscribir}
        onKeyDown={enTecla}
        role="combobox"
        aria-expanded={abierto}
        aria-controls={abierto ? idListbox : undefined}
        aria-autocomplete="list"
        aria-activedescendant={abierto && visibles[activa] ? `${idListbox}-op-${activa}` : undefined}
      />
      <button
        type="button"
        title="Autocompletar refs $. (Ctrl/Cmd+Espacio)"
        aria-label="Autocompletar refs $."
        class="absolute right-1.5 top-1.5 rounded-[5px] border border-borde-fuerte bg-superficie px-1.5 py-0.5 font-mono text-[11px] text-tinta-suave hover:border-tinta-suave hover:text-tinta"
        onMouseDown={(e) => e.preventDefault()}
        onClick={abrir}
      >
        $
      </button>
      {abierto && (
        <ul
          id={idListbox}
          class="absolute z-20 mt-1 max-h-64 w-full overflow-auto rounded-[7px] border border-borde-fuerte bg-superficie py-1 shadow-lg"
          role="listbox"
          aria-label="Referencias disponibles"
          onMouseDown={(e) => e.preventDefault()}
        >
          {visibles.length === 0 && (
            <li class="px-3 py-1.5 text-[12.5px] text-tinta-suave">Ninguna ref coincide con «{filtro}».</li>
          )}
          {visibles.map((s, i) => (
            <li key={s.ruta} id={`${idListbox}-op-${i}`}>
              <button
                type="button"
                class={`flex w-full items-center justify-between gap-2 px-3 py-1.5 text-left font-mono text-[12.5px] ${i === activa ? "bg-tinta/5 text-tinta" : "text-tinta-suave hover:bg-tinta/5"}`}
                role="option"
                aria-selected={i === activa}
                onMouseEnter={() => setActiva(i)}
                onClick={() => elegir(s.ruta)}
              >
                <span>{s.ruta}</span>
                <span class="shrink-0 rounded-[4px] bg-papel px-1.5 py-0.5 font-mono text-[10px] text-tinta-tenue">
                  {s.fuente}
                </span>
              </button>
            </li>
          ))}
          {ocultas > 0 && (
            <li class="px-3 py-1 text-[11.5px] text-tinta-tenue">+{ocultas} más: sigue escribiendo para filtrar</li>
          )}
        </ul>
      )}
    </div>
  );
}
