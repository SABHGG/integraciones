//! Página de miembros (MVP4): lista visible por cualquier identidad
//! autenticada; añadir/cambiar rol/expulsar solo para `owner`, con los
//! errores del servidor en línea (una sola fuente de verdad).
//! Render El rastro (MVP6): misma lógica; presentación con ui.tsx.
import { useEffect, useState } from "preact/hooks";
import { api, type Page } from "./api";
import { puede, type Rol } from "./roles";
import { Boton, Campo, Control, Esqueleto, Panel } from "./ui";

interface Miembro {
  userId: string;
  email: string;
  role: Rol;
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

/** Rol del miembro: valor del sistema, en mono y sin color (no es un estado). */
function InsigniaRol(props: { rol: string }) {
  return (
    <span class="rounded-[4px] border border-borde-fuerte bg-superficie px-2 py-1 font-mono text-[11.5px] text-tinta-suave">
      {props.rol}
    </span>
  );
}

export function Miembros(props: { rol: Rol | null }) {
  const [miembros, setMiembros] = useState<Miembro[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [email, setEmail] = useState("");
  const [rolNuevo, setRolNuevo] = useState("editor");
  const [cargando, setCargando] = useState(true);

  const gestionar = puede(props.rol, "gestionar-miembros");

  const cargar = async () => {
    setError(null);
    try {
      const pagina = await api<Page<Miembro>>("GET", "/members");
      setMiembros(pagina.items);
    } catch (e) {
      setError(e instanceof Error ? e.message : "error al listar");
    } finally {
      setCargando(false);
    }
  };

  useEffect(() => {
    void cargar();
  }, []);

  const anadir = async (evento: Event) => {
    evento.preventDefault();
    setAviso(null);
    setError(null);
    try {
      await api("POST", "/members", { email: email.trim(), role: rolNuevo });
      setEmail("");
      setAviso("miembro añadido");
      void cargar();
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo añadir");
    }
  };

  const cambiarRol = async (userId: string, role: string) => {
    setAviso(null);
    setError(null);
    try {
      await api("PATCH", `/members/${userId}`, { role });
      setAviso("rol actualizado");
      void cargar();
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo cambiar el rol");
    }
  };

  const expulsar = async (userId: string) => {
    setAviso(null);
    setError(null);
    try {
      await api("DELETE", `/members/${userId}`);
      setAviso("miembro expulsado");
      void cargar();
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo expulsar");
    }
  };

  const columnas = gestionar ? 4 : 3;

  return (
    <div class="space-y-3">
      <div class="flex flex-wrap items-baseline gap-x-4 gap-y-2">
        <h1 class="text-[21px] font-bold tracking-tight text-tinta">Miembros</h1>
        <span class="hidden text-[13.5px] text-tinta-suave lg:inline">
          Personas con acceso a este workspace y su rol
        </span>
      </div>

      {aviso && <Aviso texto={aviso} />}
      {error && <Aviso texto={error} tono="mal" />}

      {gestionar && (
        <Panel class="p-4">
          <form onSubmit={anadir}>
            <div class="text-[14px] font-semibold text-tinta">Añadir miembro</div>
            <p class="mt-1 mb-3 text-[12.5px] text-tinta-suave">
              La persona ya debe tener cuenta; al añadirla obtiene acceso con el rol que elijas.
            </p>
            <div class="grid gap-x-4 gap-y-3 md:grid-cols-2">
              <Campo etiqueta="Email">
                <Control
                  id="email-nuevo"
                  type="email"
                  value={email}
                  placeholder="persona@dominio"
                  onInput={(e: Event) => setEmail((e.target as HTMLInputElement).value)}
                />
              </Campo>
              <Campo etiqueta="Rol">
                <Control
                  as="select"
                  id="rol-nuevo"
                  value={rolNuevo}
                  onChange={(e: Event) => setRolNuevo((e.target as HTMLSelectElement).value)}
                >
                  <option value="editor">editor</option>
                  <option value="runner">runner</option>
                  <option value="viewer">viewer</option>
                  <option value="owner">owner</option>
                </Control>
              </Campo>
            </div>
            <div class="mt-3">
              <Boton type="submit" deshabilitado={email.trim() === ""}>Añadir</Boton>
            </div>
          </form>
        </Panel>
      )}

      <Panel class="overflow-hidden">
        <div class="overflow-x-auto">
          <table class="w-full min-w-[640px] border-collapse">
            <thead>
              <tr class="border-b border-borde text-left text-[12px] font-semibold text-tinta-suave">
                <th class="px-4 py-2.5 font-semibold">Email</th>
                <th class="px-4 py-2.5 font-semibold">Rol</th>
                <th class="px-4 py-2.5 font-semibold">Alta</th>
                {gestionar && <th class="px-4 py-2.5 font-semibold">Acciones</th>}
              </tr>
            </thead>
            <tbody>
              {miembros.map((miembro) => (
                <tr key={miembro.userId} class="border-b border-borde last:border-0">
                  <td class="px-4 py-2.5 text-[13.5px]">{miembro.email}</td>
                  <td class="px-4 py-2.5">
                    {gestionar ? (
                      <Control
                        as="select"
                        class="w-auto!"
                        aria-label={`Rol de ${miembro.email}`}
                        value={miembro.role}
                        onChange={(e: Event) =>
                          void cambiarRol(miembro.userId, (e.target as HTMLSelectElement).value)
                        }
                      >
                        <option value="owner">owner</option>
                        <option value="editor">editor</option>
                        <option value="runner">runner</option>
                        <option value="viewer">viewer</option>
                      </Control>
                    ) : (
                      <InsigniaRol rol={miembro.role} />
                    )}
                  </td>
                  <td class="px-4 py-2.5 text-[13px] text-tinta-suave">
                    {new Date(miembro.createdAt).toLocaleDateString()}
                  </td>
                  {gestionar && (
                    <td class="px-4 py-2.5 text-right">
                      <Boton
                        variante="peligro"
                        chico
                        onClick={() => void expulsar(miembro.userId)}
                        title={`Expulsa a ${miembro.email} del workspace`}
                      >
                        Expulsar
                      </Boton>
                    </td>
                  )}
                </tr>
              ))}
              {cargando && miembros.length === 0 && (
                <tr>
                  <td colspan={columnas}>
                    <Esqueleto filas={3} />
                  </td>
                </tr>
              )}
              {!cargando && miembros.length === 0 && !error && (
                <tr>
                  <td colspan={columnas} class="px-4 py-8 text-center text-[13.5px] text-tinta-suave">
                    {gestionar
                      ? "Aún no hay miembros. Añade a la primera persona con el formulario de arriba."
                      : "Aún no hay miembros en este workspace."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        {!gestionar && (
          <p class="border-t border-borde px-4 py-2.5 text-[12.5px] text-tinta-tenue">
            Solo un owner puede añadir miembros, cambiar roles o expulsarlos.
          </p>
        )}
      </Panel>
    </div>
  );
}
