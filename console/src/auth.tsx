//! Login de la consola (MVP4): email+contraseña (primario, con token de
//! sesión) o API key (acceso alternativo de automatización/demo). Si el
//! usuario pertenece a varios workspaces, se elige cuál fijar en la sesión.
//! Rediseño MVP6: sistema visual El rastro (ui.tsx); lógica intacta.
import { useState } from "preact/hooks";
import { entrar, setApiKey, type SesionLogin } from "./api";
import { Boton, Campo, Control, Panel } from "./ui";

interface Props {
  alEntrar: (rol: string) => void;
}

export function Login(props: Props) {
  const [modo, setModo] = useState<"usuario" | "api_key">("usuario");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [clave, setClave] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [cargando, setCargando] = useState(false);
  const [pendiente, setPendiente] = useState<SesionLogin | null>(null);

  const completar = (sesion: SesionLogin) => {
    props.alEntrar(sesion.activeWorkspace?.role ?? "viewer");
  };

  const entrarConCredenciales = async (workspace?: string) => {
    setCargando(true);
    setError(null);
    try {
      const sesion = await entrar(email.trim(), password, workspace);
      if (sesion.activeWorkspace) {
        completar(sesion);
      } else {
        // Multi-workspace: elegir cuál fijar (re-login con el slug).
        setPendiente(sesion);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo entrar");
    } finally {
      setCargando(false);
    }
  };

  const entrarConKey = async (evento: Event) => {
    evento.preventDefault();
    setCargando(true);
    setError(null);
    setApiKey(clave.trim());
    // Validamos la key contra el plano: si es inválida, se muestra su error.
    try {
      const { api } = await import("./api");
      await api("GET", "/bridges?limit=1");
      props.alEntrar("owner");
    } catch (e) {
      setApiKey("");
      setError(e instanceof Error ? e.message : "no se pudo validar la key");
    } finally {
      setCargando(false);
    }
  };

  const enviarCredenciales = async (evento: Event) => {
    evento.preventDefault();
    await entrarConCredenciales();
  };

  if (pendiente) {
    // Elección de workspace: la sesión sin fijar exige re-login con slug.
    return (
      <div class="flex min-h-screen flex-col items-center bg-papel px-4 pt-[16vh]">
        <h1 class="mb-5 px-1 text-[22px] font-bold tracking-tight text-tinta">integraciones</h1>
        <Panel class="w-full max-w-[420px] p-6">
          <h2 class="text-[16px] font-semibold text-tinta">Elige un workspace</h2>
          <p class="mt-1 text-[13px] text-tinta-suave">Perteneces a varios: la sesión se fijará al que elijas.</p>
          <div class="mt-5 flex flex-col gap-2">
            {pendiente.workspaces.map((w) => (
              <Boton
                key={w.slug}
                variante="secundario"
                class="justify-between"
                onClick={() => void entrarConCredenciales(w.slug)}
              >
                <span class="truncate">{w.name}</span>
                <span class="ml-3 shrink-0 font-mono text-[11.5px] text-tinta-suave">
                  {w.slug} ({w.role})
                </span>
              </Boton>
            ))}
          </div>
          {error && (
            <p class="mt-4 rounded-[6px] border border-mal/40 bg-mal/5 px-3 py-2 text-[13px] text-mal">{error}</p>
          )}
        </Panel>
      </div>
    );
  }

  return (
    <div class="flex min-h-screen flex-col items-center bg-papel px-4 pt-[16vh]">
      <h1 class="mb-5 px-1 text-[22px] font-bold tracking-tight text-tinta">integraciones</h1>
      <Panel class="w-full max-w-[420px] p-6">
        <form onSubmit={modo === "usuario" ? enviarCredenciales : entrarConKey}>
          {modo === "usuario" ? (
            <>
              <Campo etiqueta="Email">
                <Control
                  id="email"
                  type="email"
                  autocomplete="username"
                  value={email}
                  onInput={(e: Event) => setEmail((e.target as HTMLInputElement).value)}
                />
              </Campo>
              <Campo etiqueta="Contraseña" class="mt-4">
                <Control
                  id="password"
                  type="password"
                  autocomplete="current-password"
                  value={password}
                  onInput={(e: Event) => setPassword((e.target as HTMLInputElement).value)}
                />
              </Campo>
              <p class="mt-4 text-[12.5px] leading-relaxed text-tinta-suave">
                ¿Sin cuenta? Regístrate por API:{" "}
                <code class="font-mono text-[11.5px] text-tinta">POST /api/v1/auth/register</code>
              </p>
            </>
          ) : (
            <>
              <Campo etiqueta="API key del workspace">
                <Control
                  id="clave"
                  type="password"
                  autocomplete="current-password"
                  value={clave}
                  placeholder="ik_…"
                  onInput={(e: Event) => setClave((e.target as HTMLInputElement).value)}
                />
              </Campo>
              <p class="mt-4 text-[12.5px] leading-relaxed text-tinta-suave">
                Acceso de automatización y demo: actúa como owner.
              </p>
            </>
          )}
          {error && (
            <p class="mt-4 rounded-[6px] border border-mal/40 bg-mal/5 px-3 py-2 text-[13px] text-mal">{error}</p>
          )}
          <div class="mt-6 flex items-center gap-2">
            <Boton type="submit" deshabilitado={cargando}>
              {cargando ? "Validando…" : modo === "usuario" ? "Entrar" : "Entrar con key"}
            </Boton>
            <Boton
              variante="secundario"
              onClick={() => {
                setModo(modo === "usuario" ? "api_key" : "usuario");
                setError(null);
              }}
            >
              {modo === "usuario" ? "Usar API key" : "Usar email y contraseña"}
            </Boton>
          </div>
        </form>
      </Panel>
    </div>
  );
}
