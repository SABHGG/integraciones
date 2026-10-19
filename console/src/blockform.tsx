//! Formulario de configuración generado desde los metadatos `ui` de la
//! versión del bloque; sin `ui` → editor JSON genérico con validación del
//! servidor al guardar.
import { camposDeUi, valorDeCampo, type UiMeta } from "./ui-forms";

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
        <label>config (JSON)</label>
        <textarea
          value={JSON.stringify(props.config, null, 2)}
          onInput={(e) => {
            const crudo = (e.target as HTMLTextAreaElement).value;
            try {
              props.onChange(JSON.parse(crudo));
            } catch {
              // JSON incompleto mientras se escribe: no se notifica aún.
            }
          }}
          rows={6}
        />
        {(props.errores ?? []).map((issue) => (
          <p class="error-campo">{issue.message}</p>
        ))}
      </div>
    );
  }

  return (
    <div>
      {campos.map((campo) => {
        const crudo = props.config[campo.nombre];
        const valor = crudo === undefined || crudo === null ? "" : String(crudo);
        const issue = (props.errores ?? []).find((e) => e.field === `config.${campo.nombre}`);
        return (
          <div>
            <label for={`campo-${campo.nombre}`}>{campo.nombre}</label>
            <input
              id={`campo-${campo.nombre}`}
              type={campo.widget === "number" ? "number" : campo.widget === "password" ? "password" : "text"}
              value={valor}
              onInput={(e) => {
                const nuevo = { ...props.config };
                nuevo[campo.nombre] = valorDeCampo(campo.widget, (e.target as HTMLInputElement).value);
                props.onChange(nuevo);
              }}
            />
            {campo.ayuda && <p class="ayuda">{campo.ayuda}</p>}
            {issue && <p class="error-campo">{issue.message}</p>}
          </div>
        );
      })}
    </div>
  );
}
