//! Vista de versiones del puente (MVP5, tasks 4.2-4.3): lista paginada,
//! diff lado a lado (versión↔versión y borrador↔versión) con el diff
//! estructural en cliente, adopción del contenido publicado al borrador y
//! rollback orquestado (definición → PATCH del borrador → publicar).
//! Lectura para todos; adoptar/rollback solo para `editor` (D8).
import { useEffect, useState } from "preact/hooks";
import { api, type Page } from "./api";
import { diffEstructural, resumenDiff } from "./diff";
import { artefactoExport } from "./transferir";

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
      setAviso(`contenido de ${etiqueta(ladoB)} adoptado al borrador (sin publicar)`);
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
        `versión ${publicada.version} publicada: rollback de la ${ladoB} — las versiones previas y sus ejecuciones quedan intactas`,
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

  const selector = (valor: Lado, alCambiar: (lado: Lado) => void) => (
    <select
      value={String(valor)}
      onChange={(e) => {
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
    </select>
  );

  return (
    <div class="tarjeta">
      <h3>versiones</h3>
      {error && <p class="error-global">{error}</p>}
      {aviso && <p class="ayuda">{aviso}</p>}

      <table>
        <thead>
          <tr><th>versión</th><th>creada</th><th></th></tr>
        </thead>
        <tbody>
          {versiones.map((version) => (
            <tr key={version.id}>
              <td>v{version.version}</td>
              <td>{new Date(version.createdAt).toLocaleString()}</td>
              <td>
                <button
                  class="secundario"
                  onClick={() => exportarVersion(version.version)}
                  title="descarga {name, slug, definition} congelado"
                >
                  exportar
                </button>
              </td>
            </tr>
          ))}
          {versiones.length === 0 && <tr><td colspan={3}>sin versiones publicadas</td></tr>}
        </tbody>
      </table>

      <h4>comparar</h4>
      <div class="acciones">
        {selector(ladoA, setLadoA)}
        <span>contra</span>
        {selector(ladoB, setLadoB)}
        <span class="insignia">{resumenDiff(lineas)}</span>
      </div>

      {lineas.length > 0 && (
        <table>
          <thead>
            <tr><th>ruta</th><th>{etiqueta(ladoA)}</th><th>{etiqueta(ladoB)}</th></tr>
          </thead>
          <tbody>
            {lineas.map((linea) => (
              <tr key={linea.ruta}>
                <td><code>{linea.ruta}</code></td>
                <td>{JSON.stringify(linea.izq)}</td>
                <td>{JSON.stringify(linea.der)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {props.administra && (
        <div class="acciones" style="margin-top: 8px;">
          <button class="secundario" onClick={() => void adoptar()}>
            adoptar {etiqueta(ladoB)} al borrador
          </button>
          {ladoB !== "borrador" && (
            confirmando ? (
              <>
                <button class="peligro" onClick={() => void rollback()}>
                  confirmar rollback a la {ladoB}
                </button>
                <button class="secundario" onClick={() => setConfirmando(false)}>
                  cancelar
                </button>
              </>
            ) : (
              <button class="peligro" onClick={() => setConfirmando(true)}>
                rollback a la {ladoB}
              </button>
            )
          )}
        </div>
      )}
      {!props.administra && (
        <p class="ayuda">solo un editor puede adoptar contenido o hacer rollback</p>
      )}
    </div>
  );
}
