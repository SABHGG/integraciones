//! Página de credenciales (MVP5, tasks 2.1-2.2): listado con versión
//! vigente, crear, rotar y eliminar con confirmación escrita, historial
//! de versiones (ids y fechas, JAMÁS valores). Edición solo para
//! `editor`; viewer lee. Errores del servidor en línea.
//! Render El rastro (MVP6): misma lógica; presentación con ui.tsx.
import { useEffect, useState } from "preact/hooks";
import { api } from "../lib/api";
import { puede, type Rol } from "../lib/roles";
import {
  validarCreacion,
  validarRotacion,
  confirmarEliminacion,
  esVigente,
  type ProblemaCampo,
} from "../lib/credenciales";
import { Boton, Campo, Control, Esqueleto, Panel } from "../ui";

interface Credencial {
  id: string;
  name: string;
  description: string;
  currentVersionId: string | null;
  createdAt: string;
}

interface VersionCredencial {
  id: string;
  version: number;
  createdAt: string;
}

/** Aviso de la página: verde para lo hecho, ladrillo para el error del servidor. */
function Aviso(props: { texto: string; tono?: "ok" | "mal" }) {
  const clase =
    props.tono === "mal"
      ? "border-mal/40 bg-mal/5 text-mal"
      : "border-ok/30 bg-ok-fondo text-ok";
  return (
    <div class={`rounded-[8px] border px-3.5 py-2.5 text-[13.5px] ${clase}`}>{props.texto}</div>
  );
}

/** Estado de la versión: valor del sistema, en mono y sin color (no es un paso). */
function Etiqueta(props: { texto: string }) {
  return (
    <span class="rounded-[4px] border border-borde-fuerte bg-superficie px-2 py-1 font-mono text-[11.5px] text-tinta-suave">
      {props.texto}
    </span>
  );
}

export function Credenciales(props: { rol: Rol | null }) {
  const [lista, setLista] = useState<Credencial[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [problemas, setProblemas] = useState<ProblemaCampo[]>([]);
  const [cargando, setCargando] = useState(true);

  // Formulario de creación
  const [nombre, setNombre] = useState("");
  const [secreto, setSecreto] = useState("");
  const [descripcion, setDescripcion] = useState("");

  // Rotación y eliminación por credencial
  const [rotando, setRotando] = useState<Record<string, string>>({});
  const [borrando, setBorrando] = useState<Record<string, string>>({});
  // Historial abierto por credencial
  const [historial, setHistorial] = useState<Record<string, VersionCredencial[] | null>>({});

  const administra = puede(props.rol, "administrar-recursos");

  const cargar = async () => {
    setError(null);
    try {
      // Contrato del plano: la bóveda no pagina por cursor; devuelve {credentials}.
      const { credentials } = await api<{ credentials: Credencial[] }>("GET", "/credentials");
      setLista(credentials);
    } catch (e) {
      setError(e instanceof Error ? e.message : "error al listar credenciales");
    } finally {
      setCargando(false);
    }
  };

  useEffect(() => {
    void cargar();
  }, []);

  const crear = async (evento: Event) => {
    evento.preventDefault();
    setAviso(null);
    const issues = validarCreacion({ name: nombre, secret: secreto });
    setProblemas(issues);
    if (issues.length > 0) return;
    try {
      await api("POST", "/credentials", {
        name: nombre.trim(),
        secret: secreto,
        description: descripcion.trim(),
      });
      setNombre("");
      setSecreto("");
      setDescripcion("");
      setAviso("credencial creada (versión 1 vigente)");
      void cargar();
    } catch (e) {
      if (e instanceof Error && "fields" in e) {
        setProblemas((e as { fields: ProblemaCampo[] }).fields);
      }
      setError(e instanceof Error ? e.message : "no se pudo crear");
    }
  };

  const rotar = async (id: string) => {
    setAviso(null);
    setError(null);
    const valor = rotando[id] ?? "";
    const issues = validarRotacion(valor);
    setProblemas(issues);
    if (issues.length > 0) return;
    try {
      await api("PATCH", `/credentials/${id}`, { secret: valor });
      setRotando((previo) => ({ ...previo, [id]: "" }));
      setAviso("credencial rotada: nueva versión vigente");
      void cargar();
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo rotar");
    }
  };

  const eliminar = async (credencial: Credencial) => {
    setAviso(null);
    setError(null);
    if (!confirmarEliminacion(credencial.name, borrando[credencial.id] ?? "")) return;
    try {
      await api("DELETE", `/credentials/${credencial.id}`);
      setBorrando((previo) => ({ ...previo, [credencial.id]: "" }));
      setAviso("credencial eliminada");
      void cargar();
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo eliminar");
    }
  };

  const abrirHistorial = async (credencial: Credencial) => {
    setAviso(null);
    if (historial[credencial.id]) {
      // Ya abierto: cerrar.
      setHistorial((previo) => ({ ...previo, [credencial.id]: null }));
      return;
    }
    try {
      const { versions } = await api<{ versions: VersionCredencial[] }>(
        "GET",
        `/credentials/${credencial.id}/versions`,
      );
      setHistorial((previo) => ({ ...previo, [credencial.id]: versions }));
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo abrir el historial");
    }
  };

  const problemaDe = (campo: string) =>
    problemas.find((p) => p.field === campo)?.message;

  const columnas = administra ? 5 : 4;

  return (
    <div class="space-y-3">
      <div class="flex flex-wrap items-baseline gap-x-4 gap-y-2">
        <h1 class="text-[21px] font-bold tracking-tight text-tinta">Credenciales</h1>
        <span class="hidden text-[13.5px] text-tinta-suave lg:inline">
          Bóveda write-only: los valores jamás se muestran ni se descargan; solo se crean y se rotan.
          Cada ejecución audita qué versión usó.
        </span>
      </div>

      {aviso && <Aviso texto={aviso} />}
      {error && <Aviso texto={error} tono="mal" />}

      {administra && (
        <Panel class="p-4">
          <form onSubmit={crear}>
            <div class="text-[14px] font-semibold text-tinta">Nueva credencial</div>
            <p class="mt-1 mb-3 text-[12.5px] text-tinta-suave">
              El secreto se cifra en reposo y nunca se vuelve a mostrar: si se filtra, rótalo.
            </p>
            <div class="grid gap-x-4 gap-y-3 md:grid-cols-2">
              <Campo etiqueta="Nombre" error={problemaDe("name")}>
                <Control
                  id="cred-nombre"
                  value={nombre}
                  onInput={(e: Event) => setNombre((e.target as HTMLInputElement).value)}
                />
              </Campo>
              <Campo etiqueta="Secreto" ayuda="Se cifra en reposo." error={problemaDe("secret")}>
                <Control
                  id="cred-secreto"
                  type="password"
                  autocomplete="new-password"
                  value={secreto}
                  onInput={(e: Event) => setSecreto((e.target as HTMLInputElement).value)}
                />
              </Campo>
              <Campo etiqueta="Descripción" class="md:col-span-2">
                <Control
                  id="cred-desc"
                  value={descripcion}
                  onInput={(e: Event) => setDescripcion((e.target as HTMLInputElement).value)}
                />
              </Campo>
            </div>
            <div class="mt-3">
              <Boton type="submit">Crear credencial</Boton>
            </div>
          </form>
        </Panel>
      )}

      <Panel class="overflow-hidden">
        <div class="overflow-x-auto">
          <table class="w-full min-w-[1080px] border-collapse">
            <thead>
              <tr class="border-b border-borde text-left text-[12px] font-semibold text-tinta-suave">
                <th class="px-4 py-2.5 font-semibold">Nombre</th>
                <th class="px-4 py-2.5 font-semibold">Descripción</th>
                <th class="px-4 py-2.5 font-semibold">Versión vigente</th>
                <th class="px-4 py-2.5 font-semibold">Alta</th>
                {administra && <th class="px-4 py-2.5 font-semibold">Acciones</th>}
              </tr>
            </thead>
            <tbody>
              {lista.map((credencial) => (
                <tr key={credencial.id} class="border-b border-borde last:border-0">
                  <td class="px-4 py-2.5 text-[13.5px]">{credencial.name}</td>
                  <td class="px-4 py-2.5 text-[13px] text-tinta-suave">{credencial.description}</td>
                  <td class="px-4 py-2.5">
                    {credencial.currentVersionId ? (
                      <Etiqueta texto="vigente" />
                    ) : (
                      <Etiqueta texto="sin versión" />
                    )}
                  </td>
                  <td class="px-4 py-2.5 text-[13px] text-tinta-suave">
                    {new Date(credencial.createdAt).toLocaleDateString()}
                  </td>
                  {administra && (
                    <td class="px-4 py-2.5">
                      <div class="flex flex-wrap items-center gap-2">
                        <Control
                          type="password"
                          class="max-w-[150px]"
                          autocomplete="new-password"
                          placeholder="Valor nuevo"
                          aria-label={`Valor nuevo para ${credencial.name}`}
                          value={rotando[credencial.id] ?? ""}
                          onInput={(e: Event) =>
                            setRotando((previo) => ({
                              ...previo,
                              [credencial.id]: (e.target as HTMLInputElement).value,
                            }))
                          }
                        />
                        <Boton
                          variante="secundario"
                          chico
                          onClick={() => void rotar(credencial.id)}
                          title="Publica una versión nueva y la deja vigente"
                        >
                          Rotar
                        </Boton>
                        <Boton
                          variante="secundario"
                          chico
                          onClick={() => void abrirHistorial(credencial)}
                          title="Versiones anteriores (ids y fechas, sin valores)"
                        >
                          Historial
                        </Boton>
                        <Control
                          class="max-w-[170px]"
                          placeholder={`Teclea «${credencial.name}»`}
                          aria-label={`Escribe el nombre para confirmar la eliminación de ${credencial.name}`}
                          value={borrando[credencial.id] ?? ""}
                          onInput={(e: Event) =>
                            setBorrando((previo) => ({
                              ...previo,
                              [credencial.id]: (e.target as HTMLInputElement).value,
                            }))
                          }
                        />
                        <Boton
                          variante="peligro"
                          chico
                          deshabilitado={
                            !confirmarEliminacion(credencial.name, borrando[credencial.id] ?? "")
                          }
                          onClick={() => void eliminar(credencial)}
                          title="Eliminar es irreversible; las ejecuciones pasadas conservan su traza"
                        >
                          Eliminar
                        </Boton>
                      </div>
                    </td>
                  )}
                </tr>
              ))}
              {cargando && lista.length === 0 && (
                <tr>
                  <td colspan={columnas}>
                    <Esqueleto filas={4} />
                  </td>
                </tr>
              )}
              {!cargando && lista.length === 0 && !error && (
                <tr>
                  <td colspan={columnas} class="px-4 py-8 text-center text-[13.5px] text-tinta-suave">
                    {administra
                      ? "Aún no hay credenciales. Crea la primera con el formulario de arriba."
                      : "Aún no hay credenciales en este workspace."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        {!administra && (
          <p class="border-t border-borde px-4 py-2.5 text-[12.5px] text-tinta-tenue">
            Solo un editor puede crear, rotar o eliminar credenciales.
          </p>
        )}
      </Panel>

      {lista.map((credencial) =>
        historial[credencial.id] ? (
          <Panel key={`hist-${credencial.id}`}>
            <div class="flex flex-wrap items-center gap-3 border-b border-borde px-4 py-3">
              <span class="text-[14px] font-semibold text-tinta">
                Historial de {credencial.name}
              </span>
              <Boton
                variante="secundario"
                chico
                class="ml-auto"
                onClick={() => void abrirHistorial(credencial)}
              >
                Cerrar
              </Boton>
            </div>
            <div class="overflow-x-auto">
              <table class="w-full min-w-[420px] border-collapse">
                <thead>
                  <tr class="border-b border-borde text-left text-[12px] font-semibold text-tinta-suave">
                    <th class="px-4 py-2 font-semibold">Versión</th>
                    <th class="px-4 py-2 font-semibold">Creada</th>
                    <th class="px-4 py-2 font-semibold">Estado</th>
                  </tr>
                </thead>
                <tbody>
                  {(historial[credencial.id] ?? []).map((version) => (
                    <tr key={version.id} class="border-b border-borde last:border-0">
                      <td class="px-4 py-2 font-mono text-[12.5px]">v{version.version}</td>
                      <td class="px-4 py-2 text-[13px] text-tinta-suave">
                        {new Date(version.createdAt).toLocaleString()}
                      </td>
                      <td class="px-4 py-2">
                        {esVigente(version.id, credencial.currentVersionId) ? (
                          <Etiqueta texto="vigente" />
                        ) : (
                          <span class="text-[13px] text-tinta-suave">reemplazada</span>
                        )}
                      </td>
                    </tr>
                  ))}
                  {(historial[credencial.id] ?? []).length === 0 && (
                    <tr>
                      <td colspan={3} class="px-4 py-4 text-[13.5px] text-tinta-suave">
                        Todavía no hay versiones registradas para esta credencial.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
            <p class="border-t border-borde px-4 py-2.5 text-[12.5px] text-tinta-tenue">
              Solo ids y fechas: los valores nunca salen de la bóveda.
            </p>
          </Panel>
        ) : null,
      )}
    </div>
  );
}
