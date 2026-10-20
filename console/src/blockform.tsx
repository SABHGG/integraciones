//! Formulario de configuración generado desde los metadatos `ui` de la
//! versión del bloque; sin `ui` → editor JSON genérico con validación del
//! servidor al guardar. Render El rastro (MVP6): los valores de configuración
//! van en mono; etiquetas visibles siempre.
import { camposDeUi, valorDeCampo, type UiMeta } from "./ui-forms";
import { Campo, Control } from "./ui";

export function FormularioInstancia(props: {
  ui: UiMeta | null;
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  errores?: { field: string; message: string }[];
}) {
  const campos = camposDeUi(props.ui);

  if (!campos) {
    // Bloque sin metadatos `ui`: JSON libre (el servidor valida contra
    // configSchema/inputSchema al guardar o publicar).
    return (
      <div>
        <Campo etiqueta="Config (JSON)" ayuda="El servidor valida contra configSchema al guardar.">
          <Control
            as="textarea"
            mono
            rows={6}
            value={JSON.stringify(props.config, null, 2)}
            onInput={(e: Event) => {
              const crudo = (e.target as HTMLTextAreaElement).value;
              try {
                props.onChange(JSON.parse(crudo));
              } catch {
                // JSON incompleto mientras se escribe: no se notifica aún.
              }
            }}
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
      {campos.map((campo) => {
        const crudo = props.config[campo.nombre];
        const valor = crudo === undefined || crudo === null ? "" : String(crudo);
        const issue = (props.errores ?? []).find((e) => e.field === `config.${campo.nombre}`);
        return (
          <div key={campo.nombre}>
            <Campo etiqueta={campo.nombre} error={issue?.message} ayuda={campo.ayuda}>
              <Control
                id={`campo-${campo.nombre}`}
                type={campo.widget === "number" ? "number" : campo.widget === "password" ? "password" : "text"}
                value={valor}
                onInput={(e: Event) => {
                  const nuevo = { ...props.config };
                  nuevo[campo.nombre] = valorDeCampo(campo.widget, (e.target as HTMLInputElement).value);
                  props.onChange(nuevo);
                }}
              />
            </Campo>
          </div>
        );
      })}
    </div>
  );
}
