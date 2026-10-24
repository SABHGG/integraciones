//! Formulario de configuración de instancias. Con metadatos `ui` genera el
//! formulario con los 7 widgets del contrato (text, url, number, textarea,
//! password, select —opciones del `enum` del configSchema—, json y
//! ref-template); sin `ui`, editor JSON genérico con autocompletado de refs
//! `$.`. El servidor valida contra configSchema/inputSchema al guardar.
//! Render El rastro (MVP6): valores en mono, etiquetas visibles siempre.
import { useState } from "preact/hooks";
import { camposDeUi, valorDeCampo, type CampoUi, type UiMeta } from "../lib/ui-forms";
import { Campo, Control } from "./ui";
import { InputConRefs } from "./input-con-refs";
import type { SugerenciaRef } from "../lib/refs-contexto";

/** Decisión widget → control, pura y testeable (5.1). */
export type WidgetRender =
  | { tipo: "input"; inputType: "text" | "url" | "number" | "password" }
  | { tipo: "textarea" }
  | { tipo: "select" }
  | { tipo: "json" }
  | { tipo: "refs" };

export function renderDeWidget(widget: string, opciones?: unknown[]): WidgetRender {
  switch (widget) {
    case "select":
      // Sin enum en el configSchema no hay opciones: cae a texto (D1).
      return opciones && opciones.length > 0 ? { tipo: "select" } : { tipo: "input", inputType: "text" };
    case "json":
      return { tipo: "json" };
    case "ref-template":
      return { tipo: "refs" };
    case "textarea":
      return { tipo: "textarea" };
    case "url":
    case "number":
    case "password":
      return { tipo: "input", inputType: widget };
    default:
      return { tipo: "input", inputType: "text" };
  }
}

export function FormularioInstancia(props: {
  ui: UiMeta | null;
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  errores?: { field: string; message: string }[];
  /** Sugerencias de refs `$.` disponibles en este punto de la cadena. */
  sugerencias?: SugerenciaRef[];
  /** configSchema de la versión (opciones de los select). */
  configSchema?: unknown;
  /** Clave de remontaje (id de instancia) para los editores json. */
  claveInstancia?: string;
}) {
  const campos = camposDeUi(props.ui, props.configSchema);

  if (!campos) {
    // Bloque sin metadatos `ui`: JSON libre con refs `$.` (el servidor valida
    // contra configSchema/inputSchema al guardar o publicar).
    return (
      <div>
        <Campo etiqueta="Config (JSON)" ayuda="El servidor valida contra configSchema al guardar. Las refs $. se insertan interpoladas {$.ref}.">
          <InputConRefs
            valor={JSON.stringify(props.config, null, 2)}
            onInput={(crudo) => {
              try {
                props.onChange(JSON.parse(crudo));
              } catch {
                // JSON incompleto mientras se escribe: no se notifica aún.
              }
            }}
            sugerencias={props.sugerencias ?? []}
            modo="interpolar"
            multilinea
            filas={6}
          />
        </Campo>
        {(props.errores ?? []).map((issue) => (
          <p class="mt-1.5 rounded-[6px] border border-mal/40 bg-mal/5 px-2 py-1 text-[12.5px] text-mal">
            {issue.message}
          </p>
        ))}
      </div>
    );
  }

  return (
    <div class="grid gap-x-4 gap-y-3 md:grid-cols-2">
      {campos.map((campo) => (
        <CampoFormulario
          key={campo.nombre}
          campo={campo}
          config={props.config}
          onChange={props.onChange}
          errores={props.errores ?? []}
          sugerencias={props.sugerencias ?? []}
          claveRemonte={`${props.claveInstancia ?? ""}-${campo.nombre}`}
        />
      ))}
    </div>
  );
}

/** Un campo del formulario según su widget. */
function CampoFormulario(props: {
  campo: CampoUi;
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  errores: { field: string; message: string }[];
  sugerencias: SugerenciaRef[];
  claveRemonte: string;
}) {
  const { campo, config, onChange } = props;
  const crudo = config[campo.nombre];
  const issue = props.errores.find((e) => e.field === `config.${campo.nombre}`);
  const error = issue?.message;
  const render = renderDeWidget(campo.widget, campo.opciones);

  const poner = (valor: unknown) => {
    onChange({ ...config, [campo.nombre]: valor });
  };

  if (render.tipo === "select") {
    const opciones = campo.opciones ?? [];
    return (
      <div>
        <Campo etiqueta={campo.nombre} error={error} ayuda={campo.ayuda}>
          <Control
            as="select"
            id={`campo-${campo.nombre}`}
            value={crudo === undefined || crudo === null ? "" : String(crudo)}
            onChange={(e: Event) => {
              const seleccion = (e.target as HTMLSelectElement).value;
              const original = opciones.find((o) => String(o) === seleccion);
              poner(original ?? seleccion);
            }}
          >
            {(crudo === undefined || crudo === null) && <option value="">—</option>}
            {opciones.map((o) => (
              <option key={String(o)} value={String(o)}>
                {String(o)}
              </option>
            ))}
          </Control>
        </Campo>
      </div>
    );
  }

  if (render.tipo === "json") {
    return (
      <div key={props.claveRemonte}>
        <Campo etiqueta={campo.nombre} error={error} ayuda={campo.ayuda ?? "JSON estructurado (objeto o array); la sintaxis se valida mientras escribes."}>
          <EditorJson
            valorInicial={crudo === undefined || crudo === null ? "{}" : JSON.stringify(crudo, null, 2)}
            onChange={(valor) => poner(valor)}
          />
        </Campo>
      </div>
    );
  }

  if (render.tipo === "refs") {
    return (
      <div>
        <Campo etiqueta={campo.nombre} error={error} ayuda={campo.ayuda ?? "Texto con refs interpoladas {$....}. Ctrl/Cmd+Espacio para autocompletar."}>
          <InputConRefs
            id={`campo-${campo.nombre}`}
            valor={crudo === undefined || crudo === null ? "" : String(crudo)}
            onInput={(nuevo) => poner(valorDeCampo("text", nuevo))}
            sugerencias={props.sugerencias}
            modo="interpolar"
            placeholder="{$.trigger.body.id}"
          />
        </Campo>
      </div>
    );
  }

  if (render.tipo === "textarea") {
    return (
      <div>
        <Campo etiqueta={campo.nombre} error={error} ayuda={campo.ayuda}>
          <Control
            id={`campo-${campo.nombre}`}
            as="textarea"
            mono
            rows={3}
            value={crudo === undefined || crudo === null ? "" : String(crudo)}
            onInput={(e: Event) => poner(valorDeCampo("text", (e.target as HTMLTextAreaElement).value))}
          />
        </Campo>
      </div>
    );
  }

  // text / url / number / password: input simple.
  return (
    <div>
      <Campo etiqueta={campo.nombre} error={error} ayuda={campo.ayuda}>
        <Control
          id={`campo-${campo.nombre}`}
          type={render.tipo === "input" ? render.inputType : "text"}
          value={crudo === undefined || crudo === null ? "" : String(crudo)}
          onInput={(e: Event) => poner(valorDeCampo(campo.widget, (e.target as HTMLInputElement).value))}
        />
      </Campo>
    </div>
  );
}

/** Editor de un campo `json`: valor estructurado, no string. La sintaxis se
 valida en cliente; el valor solo viaja cuando parsea. */
function EditorJson(props: { valorInicial: string; onChange: (valor: unknown) => void }) {
  const [crudo, setCrudo] = useState(props.valorInicial);
  const invalido = !jsonParsea(crudo);
  return (
    <div>
      <Control
        as="textarea"
        mono
        rows={4}
        value={crudo}
        aria-invalid={invalido}
        onInput={(e: Event) => {
          const texto = (e.target as HTMLTextAreaElement).value;
          setCrudo(texto);
          try {
            props.onChange(JSON.parse(texto));
          } catch {
            // JSON incompleto mientras se escribe.
          }
        }}
      />
      {invalido && (
        <span class="mt-1.5 block text-[12px] text-aviso">JSON incompleto o inválido (no se guarda hasta que parsee).</span>
      )}
    </div>
  );
}

function jsonParsea(texto: string): boolean {
  try {
    JSON.parse(texto);
    return true;
  } catch {
    return false;
  }
}
