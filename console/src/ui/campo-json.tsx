//! Editor texto-a-JSON con parseo VISIBLE (critique P0-2): mientras el texto
//! no parsea, el valor no se aplica — pero el campo lo dice (borde de error +
//! nota), en vez de tragarse la edición en silencio. Estado crudo interno: el
//! texto a medio escribir no se revierte; un cambio EXTERNO del valor
//! (rollback, recarga) sí se adopta.
import { useEffect, useState } from "preact/hooks";
import { Campo, Control } from "./ui";
import { parseSeguro, textoDe } from "../lib/parse-seguro";

export function CampoJson(props: {
  /** Valor estructurado vigente (undefined = vacío). */
  valor: unknown;
  onChange: (valor: unknown) => void;
  etiqueta: string;
  ayuda?: string;
  filas?: number;
  id?: string;
  ariaLabel?: string;
  /** Qué significa "vacío" para el padre; default undefined (p. ej. borrar `when`). */
  alVacio?: unknown;
}) {
  const [crudo, setCrudo] = useState(() => textoDe(props.valor));
  // Última serialización reconocida: lo tecleado que ya se aplicó no vuelve a
  // resetear el campo (el padre reserializa con pretty-print y no coincide).
  const [reportado, setReportado] = useState(() => textoDe(props.valor));

  useEffect(() => {
    const serial = textoDe(props.valor);
    if (serial !== reportado) {
      setReportado(serial);
      setCrudo(serial);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.valor]);

  const parseo = parseSeguro(crudo);
  const invalido = parseo.ok ? undefined : parseo.error;

  return (
    <Campo etiqueta={props.etiqueta} error={invalido} ayuda={props.ayuda}>
      <Control
        as="textarea"
        id={props.id}
        aria-label={props.ariaLabel}
        mono
        rows={props.filas ?? 3}
        aria-invalid={!parseo.ok}
        value={crudo}
        onInput={(e: Event) => {
          const texto = (e.target as HTMLTextAreaElement).value;
          setCrudo(texto);
          const p = parseSeguro(texto);
          if (p.ok) {
            const aplicado = p.valor === undefined ? props.alVacio : p.valor;
            setReportado(textoDe(aplicado));
            props.onChange(aplicado);
          }
        }}
      />
      {!parseo.ok && (
        <span class="mt-1.5 block text-[12px] text-aviso">El borrador conserva el último JSON válido.</span>
      )}
    </Campo>
  );
}
