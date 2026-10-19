//! Página de miembros (MVP4): lista visible por cualquier identidad
//! autenticada; añadir/cambiar rol/expulsar solo para `owner`, con los
//! errores del servidor en línea (una sola fuente de verdad).
import { useEffect, useState } from "preact/hooks";
import { api, type Page } from "./api";
import { puede, type Rol } from "./roles";

interface Miembro {
  userId: string;
  email: string;
  role: Rol;
  createdAt: string;
}

export function Miembros(props: { rol: Rol | null }) {
  const [miembros, setMiembros] = useState<Miembro[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [email, setEmail] = useState("");
  const [rolNuevo, setRolNuevo] = useState("editor");

  const gestionar = puede(props.rol, "gestionar-miembros");

  const cargar = async () => {
    setError(null);
    try {
      const pagina = await api<Page<Miembro>>("GET", "/members");
      setMiembros(pagina.items);
    } catch (e) {
      setError(e instanceof Error ? e.message : "error al listar");
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

  return (
    <div>
      <h1>Miembros</h1>
      {aviso && <p class="ayuda">{aviso}</p>}
      {error && <p class="error-global">{error}</p>}

      {gestionar && (
        <form class="tarjeta" onSubmit={anadir}>
          <h3>añadir miembro</h3>
          <label for="email-nuevo">email (la persona debe tener cuenta)</label>
          <input id="email-nuevo" type="email" value={email}
            onInput={(e) => setEmail((e.target as HTMLInputElement).value)} placeholder="persona@dominio" />
          <label for="rol-nuevo">rol</label>
          <select id="rol-nuevo" value={rolNuevo}
            onChange={(e) => setRolNuevo((e.target as HTMLSelectElement).value)}>
            <option value="editor">editor</option>
            <option value="runner">runner</option>
            <option value="viewer">viewer</option>
            <option value="owner">owner</option>
          </select>
          <div class="acciones">
            <button type="submit" disabled={email.trim() === ""}>añadir</button>
          </div>
        </form>
      )}

      <div class="tarjeta">
        <table>
          <thead>
            <tr><th>email</th><th>rol</th><th>alta</th>{gestionar && <th>acciones</th>}</tr>
          </thead>
          <tbody>
            {miembros.map((miembro) => (
              <tr key={miembro.userId}>
                <td>{miembro.email}</td>
                <td>
                  {gestionar ? (
                    <select value={miembro.role}
                      onChange={(e) => void cambiarRol(miembro.userId, (e.target as HTMLSelectElement).value)}>
                      <option value="owner">owner</option>
                      <option value="editor">editor</option>
                      <option value="runner">runner</option>
                      <option value="viewer">viewer</option>
                    </select>
                  ) : (
                    <span class="insignia">{miembro.role}</span>
                  )}
                </td>
                <td>{new Date(miembro.createdAt).toLocaleDateString()}</td>
                {gestionar && (
                  <td>
                    <button class="peligro" onClick={() => void expulsar(miembro.userId)}>
                      expulsar
                    </button>
                  </td>
                )}
              </tr>
            ))}
            {miembros.length === 0 && <tr><td colspan={4}>sin miembros</td></tr>}
          </tbody>
        </table>
        {!gestionar && <p class="ayuda">solo un owner puede gestionar la membresía</p>}
      </div>
    </div>
  );
}
