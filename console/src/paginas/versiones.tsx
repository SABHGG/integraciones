//! Vista de versiones del puente (MVP5, tasks 4.2-4.3): lista paginada,
//! diff lado a lado (versión↔versión y borrador↔versión) con el diff
//! estructural en cliente, adopción del contenido publicado al borrador y
//! rollback orquestado (definición → PATCH del borrador → publicar).
//! Lectura para todos; adoptar/rollback solo para `editor` (D8).
//! Render El rastro (MVP6): rutas y valores del diff en mono.
import { useEffect, useState } from "preact/hooks";
import { api, type Page } from "../lib/api";
import { diffEstructural, resumenDiff } from "../lib/diff";
import { artefactoExport } from "../lib/transferir";
import { Boton, Control, Panel } from "../ui";

interface VersionMeta {
  id: string;
  version: number;
  createdAt: string;
}

interface Props {
  puenteId: string;
  /** Borrador ACTUAL del editor (para el diff y la adopción). */
  draft: unknown;
  /** Metadatos del puente para el artefacto de export (sin ids). */
  nombrePuente: string;
  slugPuente: string;
  administra: boolean;
  /** El editor recarga el puente cuando el borrador cambia en el servidor. */
  alCambiarBorrador: () => void;
}

type Lado = "borrador" | number;

export function PanelVersiones(props: Props) {
  const [versiones, setVersiones] = useState<VersionMeta[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [ladoA, setLadoA] = useState<Lado>("borrador");
  const [ladoB, setLadoB] = useState<Lado>("borrador");
  const [definiciones, setDefiniciones] = useState<Record<string, unknown>>({});
  const [confirmando, setConfirmando] = useState(false);

  const cargar = async () => {
    setError(null);
    try {
      const pagina = await api<Page<VersionMeta>>(
        "GET",
        `/bridges/${props.puenteId}/versions`,
      );
      setVersiones(pagina.items);
      if (pagina.items.length > 0) {
        setLadoB(pagina.items[0].version);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudieron listar las versiones");
    }
  };

  useEffect(() => {
    void cargar();
  }, [props.puenteId]);

  // Definición perezosa: el listado es liviano; el contenido se pide al comparar.
  useEffect(() => {
    for (const lado of [ladoA, ladoB]) {
      if (lado === "borrador") continue;
      const clave = String(lado);
      if (definiciones[clave] !== undefined) continue;
      void api<{ definition: unknown }>(
        "GET",
        `/bridges/${props.puenteId}/versions/${clave}`,
      )
        .then((detalle) =>
          setDefiniciones((previo) => ({ ...previo, [clave]: detalle.definition })),
        )
        .catch((e) =>
          setError(e instanceof Error ? e.message : "no se pudo cargar la versión"),
        );
    }
  }, [ladoA, ladoB, props.puenteId]);

  const valorDe = (lado: Lado): unknown =>
    lado === "borrador" ? props.draft : definiciones[String(lado)];

  const etiqueta = (lado: Lado): string => (lado === "borrador" ? "borrador" : `v${lado}`);

  const lineas = diffEstructural(valorDe(ladoA), valorDe(ladoB));

  const aplicarAlBorrador = async (origen: Lado) => {
    setError(null);
    setAviso(null);
    const definicion = valorDe(origen);
    if (definicion === undefined || typeof definicion !== "object" || definicion === null) {
      setError("la definición de origen aún no está cargada");
      return;
    }
    // PATCH sección por sección (contrato del borrador): solo las presentes.
    const secciones = ["trigger", "inputSchema", "blocks", "mask", "emit", "webhook"];
    const cuerpo: Record<string, unknown> = {};
    for (const clave of secciones) {
      if (clave in (definicion as Record<string, unknown>)) {
        cuerpo[clave] = (definicion as Record<string, unknown>)[clave];
      }
    }
    try {
      await api("PATCH", `/bridges/${props.puenteId}`, cuerpo);
      props.alCambiarBorrador();
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo aplicar al borrador");
      return false;
    }
  };

  const adoptar = async () => {
    if (await aplicarAlBorrador(ladoB)) {
      setAviso(`Contenido de ${etiqueta(ladoB)} adoptado al borrador (sin publicar).`);
    }
  };

  const rollback = async () => {
    if (ladoB === "borrador") {
      setError("el rollback necesita una versión de origen, no el borrador");
      return;
    }
    if (!(await aplicarAlBorrador(ladoB))) return;
    try {
      const publicada = await api<{ version: number }>(
        "POST",
        `/bridges/${props.puenteId}/versions`,
      );
      setAviso(
        `Versión ${publicada.version} publicada: rollback de la ${ladoB}. Las versiones previas y sus ejecuciones quedan intactas.`,
      );
      setConfirmando(false);
      await cargar();
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo publicar el rollback");
    }
  };

  const exportarVersion = (version: number) => {
    const definicion = definiciones[String(version)];
    if (definicion === undefined) {
      setError("la definición aún no está cargada");
      return;
    }
    const blob = new Blob(
      [artefactoExport({ name: props.nombrePuente, slug: props.slugPuente, definition: definicion })],
      { type: "application/json" },
    );
    const enlace = document.createElement("a");
    enlace.href = URL.createObjectURL(blob);
    enlace.download = `${props.slugPuente}-v${version}.puente.json`;
    enlace.click();
    URL.revokeObjectURL(enlace.href);
  };

  const selector = (valor: Lado, alCambiar: (lado: Lado) => void, etiquetaAria: string) => (
    <Control
      as="select"
      class="w-auto!"
      value={String(valor)}
      aria-label={etiquetaAria}
      onChange={(e: Event) => {
        const crudo = (e.target as HTMLSelectElement).value;
        alCambiar(crudo === "borrador" ? "borrador" : Number(crudo));
      }}
    >
      <option value="borrador">borrador</option>
      {versiones.map((version) => (
        <option key={version.id} value={version.version}>
          v{version.version}
        </option>
      ))}
    </Control>
  );

  return (
    <Panel>
      <div class="px-4 pt-3.5 text-[14px] font-semibold text-tinta">Versiones</div>
      <div class="px-4 pb-1 pt-1">
        {error && (
          <p class="rounded-[6px] border border-mal/40 bg-mal/5 px-2.5 py-1.5 text-[12.5px] text-mal">{error}</p>
        )}
        {aviso && !error && (
          <p class="rounded-[6px] border border-ok/30 bg-ok-fondo px-2.5 py-1.5 text-[12.5px] text-ok">{aviso}</p>
        )}
      </div>

      <div class="overflow-x-auto">
        <table class="w-full min-w-[420px] border-collapse">
          <thead>
            <tr class="border-b border-borde text-left text-[12px] font-semibold text-tinta-suave">
              <th class="px-4 py-2 font-semibold">Versión</th>
              <th class="px-4 py-2 font-semibold">Creada</th>
              <th class="px-4 py-2" />
            </tr>
          </thead>
          <tbody>
            {versiones.map((version) => (
              <tr key={version.id} class="border-b border-borde last:border-0">
                <td class="px-4 py-2 font-mono text-[12.5px]">v{version.version}</td>
                <td class="px-4 py-2 text-[13px] text-tinta-suave">{new Date(version.createdAt).toLocaleString()}</td>
                <td class="px-4 py-2 text-right">
                  <Boton
                    variante="secundario"
                    chico
                    onClick={() => exportarVersion(version.version)}
                    title="Descarga {name, slug, definition} congelado"
                  >
                    Exportar
                  </Boton>
                </td>
              </tr>
            ))}
            {versiones.length === 0 && (
              <tr>
                <td colspan={3} class="px-4 py-4 text-[13.5px] text-tinta-suave">
                  Sin versiones publicadas todavía. Publica el borrador para crear la primera.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      <div class="border-t border-borde px-4 py-3.5">
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-[13.5px] font-medium text-tinta">Comparar</span>
          {selector(ladoA, setLadoA, "Lado A")}
          <span class="text-[12.5px] text-tinta-tenue">contra</span>
          {selector(ladoB, setLadoB, "Lado B")}
          <span class="rounded-[4px] border border-borde-fuerte bg-papel px-2 py-1 font-mono text-[11px] text-tinta-suave">
            {resumenDiff(lineas)}
          </span>
        </div>

        {lineas.length > 0 && (
          <div class="mt-3 overflow-x-auto rounded-[6px] border border-borde">
            <table class="w-full min-w-[560px] border-collapse">
              <thead>
                <tr class="border-b border-borde bg-papel text-left text-[11.5px] font-semibold text-tinta-suave">
                  <th class="px-3 py-2 font-semibold">Ruta</th>
                  <th class="px-3 py-2 font-semibold">{etiqueta(ladoA)}</th>
                  <th class="px-3 py-2 font-semibold">{etiqueta(ladoB)}</th>
                </tr>
              </thead>
              <tbody>
                {lineas.map((linea) => (
                  <tr key={linea.ruta} class="border-b border-borde last:border-0 align-top">
                    <td class="px-3 py-2 font-mono text-[11.5px] text-tinta">{linea.ruta}</td>
                    <td class="max-w-[320px] truncate px-3 py-2 font-mono text-[11.5px] text-tinta-suave" title={JSON.stringify(linea.izq)}>
                      {JSON.stringify(linea.izq)}
                    </td>
                    <td class="max-w-[320px] truncate px-3 py-2 font-mono text-[11.5px] text-tinta" title={JSON.stringify(linea.der)}>
                      {JSON.stringify(linea.der)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {props.administra && (
          <div class="mt-3 flex flex-wrap items-center gap-2">
            <Boton variante="secundario" chico onClick={() => void adoptar()}>
              Adoptar {etiqueta(ladoB)} al borrador
            </Boton>
            {ladoB !== "borrador" && (
              confirmando ? (
                <>
                  <Boton variante="peligro" chico onClick={() => void rollback()}>
                    Confirmar rollback a la {ladoB}
                  </Boton>
                  <Boton variante="secundario" chico onClick={() => setConfirmando(false)}>
                    Cancelar
                  </Boton>
                </>
              ) : (
                <Boton variante="peligro" chico onClick={() => setConfirmando(true)}>
                  Rollback a la {ladoB}
                </Boton>
              )
            )}
          </div>
        )}
        {!props.administra && (
          <p class="mt-3 text-[12.5px] text-tinta-tenue">Solo un editor puede adoptar contenido o hacer rollback.</p>
        )}
      </div>
    </Panel>
  );
}
