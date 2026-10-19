//! Login con API key de workspace: la key se valida contra el plano admin
//! (una llamada real) y se guarda en el navegador para el Bearer.
import { useState } from "preact/hooks";
import { api, setKey } from "./api";

export function Login(props: { alEntrar: () => void }) {
  const [clave, setClave] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [cargando, setCargando] = useState(false);

  const entrar = async (evento: Event) => {
    evento.preventDefault();
    setCargando(true);
    setError(null);
    setKey(clave.trim());
    try {
      // La llamada más barata que exige autenticación: si la key es válida
      // (y pertenece a un workspace), el plano responde 200.
      await api("GET", "/bridges?limit=1");
      props.alEntrar();
    } catch (e) {
      // Key inválida o sin workspace: el error visible es el del plano admin.
      setKey("");
      setError(e instanceof Error ? e.message : "no se pudo validar la key");
    } finally {
      setCargando(false);
    }
  };

  return (
    <main>
      <form class="tarjeta" style="max-width: 420px; margin: 80px auto;" onSubmit={entrar}>
        <h2>integraciones</h2>
        <p class="ayuda">Entra con la API key de administración de tu workspace.</p>
        <label for="clave">API key</label>
        <input
          id="clave"
          type="password"
          autocomplete="current-password"
          value={clave}
          onInput={(e) => setClave((e.target as HTMLInputElement).value)}
          placeholder="ik_…"
          autofocus
        />
        {error && <p class="error-campo">{error}</p>}
        <div class="acciones">
          <button type="submit" disabled={cargando || clave.trim() === ""}>
            {cargando ? "Validando…" : "Entrar"}
          </button>
        </div>
      </form>
    </main>
  );
}
