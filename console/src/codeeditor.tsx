//! Editor de código (custom_logic) con CodeMirror 6: los errores del
//! validador del servidor (transpile/schemas) se anotan bajo el editor con
//! su línea («línea N, columna M»).
import { javascript } from "@codemirror/lang-javascript";
import { basicSetup } from "codemirror";
import { EditorView } from "@codemirror/view";
import { useEffect, useRef } from "preact/hooks";
import { lineaDeTranspilacion } from "./errors";

export function EditorCodigo(props: {
  codigo: string;
  onChange: (codigo: string) => void;
  /** Mensajes de error del servidor para anotar (puede traer «línea N»). */
  errores?: string[];
}) {
  const contenedor = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!contenedor.current) return;
    const instancia = new EditorView({
      doc: props.codigo,
      extensions: [
        basicSetup,
        javascript(),
        EditorView.updateListener.of((actualizacion) => {
          if (actualizacion.docChanged) {
            props.onChange(actualizacion.state.doc.toString());
          }
        }),
      ],
      parent: contenedor.current,
    });
    return () => instancia.destroy();
    // El editor se monta una vez por instancia; el doc cambia vía el propio
    // editor (los props iniciales no se re-aplican en cada render).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div>
      <div ref={contenedor} style="border: 1px solid var(--color-borde); border-radius: 6px;" />
      {(props.errores ?? []).map((mensaje) => {
        const linea = lineaDeTranspilacion(mensaje);
        return (
          <p class="mt-1.5 rounded-[6px] border border-mal/40 bg-mal/5 px-2 py-1 font-mono text-[12px] text-mal">
            {linea ? `línea ${linea}: ` : ""}
            {mensaje}
          </p>
        );
      })}
    </div>
  );
}
