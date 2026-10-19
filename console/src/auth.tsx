//! Login de la consola (MVP4): email+contraseña (primario, con token de
//! sesión) o API key (acceso alternativo de automatización/demo). Si el
//! usuario pertenece a varios workspaces, se elige cuál fijar en la sesión.
import { useState } from "preact/hooks";
import { entrar, setApiKey, type SesionLogin } from "./api";

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
      <main>
        <form class="tarjeta" style="max-width: 420px; margin: 80px auto;" onSubmit={(e) => e.preventDefault()}>
          <h2>elige un workspace</h2>
          <p class="ayuda">Perteneces a varios: la sesión se fijará al que elijas.</p>
          <div class="acciones" style="flex-direction: column; align-items: stretch;">
            {pendiente.workspaces.map((w) => (
              <button
                key={w.slug}
                onClick={() => void entrarConCredenciales(w.slug)}
              >
                {w.name} ({w.slug}) — {w.role}
              </button>
            ))}
          </div>
          {error && <p class="error-campo">{error}</p>}
        </form>
      </main>
    );
  }

  return (
    <main>
      <form class="tarjeta" style="max-width: 420px; margin: 80px auto;" onSubmit={modo === "usuario" ? enviarCredenciales : entrarConKey}>
        <h2>integraciones</h2>
        {modo === "usuario" ? (
          <>
            <label for="email">email</label>
            <input id="email" type="email" autocomplete="username" value={email}
              onInput={(e) => setEmail((e.target as HTMLInputElement).value)} />
            <label for="password">contraseña</label>
            <input id="password" type="password" autocomplete="current-password" value={password}
              onInput={(e) => setPassword((e.target as HTMLInputElement).value)} />
            <p class="ayuda">
              ¿Sin cuenta? Regístrate por API:
              {" "}<code>POST /api/v1/auth/register</code>
            </p>
          </>
        ) : (
          <>
            <label for="clave">API key del workspace</label>
            <input id="clave" type="password" autocomplete="current-password" value={clave}
              onInput={(e) => setClave((e.target as HTMLInputElement).value)} placeholder="ik_…" />
            <p class="ayuda">Acceso de automatización/demo: actúa como owner.</p>
          </>
        )}
        {error && <p class="error-campo">{error}</p>}
        <div class="acciones">
          <button type="submit" disabled={cargando}>
            {cargando ? "Validando…" : modo === "usuario" ? "Entrar" : "Entrar con key"}
          </button>
          <button
            type="button"
            class="secundario"
            onClick={() => {
              setModo(modo === "usuario" ? "api_key" : "usuario");
              setError(null);
            }}
          >
            {modo === "usuario" ? "usar API key" : "usar email y contraseña"}
          </button>
        </div>
      </form>
    </main>
  );
}
