//! Página de credenciales (MVP5, tasks 2.1-2.2): listado con versión
//! vigente, crear, rotar y eliminar con confirmación escrita, historial
//! de versiones (ids y fechas, JAMÁS valores). Edición solo para
//! `editor`; viewer lee. Errores del servidor en línea.
import { useEffect, useState } from "preact/hooks";
import { api, type Page } from "./api";
import { puede, type Rol } from "./roles";
import {
  validarCreacion,
  validarRotacion,
  confirmarEliminacion,
  esVigente,
  type ProblemaCampo,
} from "./credenciales";

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

export function Credenciales(props: { rol: Rol | null }) {
  const [lista, setLista] = useState<Credencial[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [problemas, setProblemas] = useState<ProblemaCampo[]>([]);

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
      const pagina = await api<Page<Credencial>>("GET", "/credentials");
      setLista(pagina.items);
    } catch (e) {
      setError(e instanceof Error ? e.message : "error al listar credenciales");
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
      const pagina = await api<Page<VersionCredencial>>(
        "GET",
        `/credentials/${credencial.id}/versions`,
      );
      setHistorial((previo) => ({ ...previo, [credencial.id]: pagina.items }));
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo abrir el historial");
    }
  };

  const problemaDe = (campo: string) =>
    problemas.find((p) => p.field === campo)?.message;

  return (
    <div>
      <h1>Credenciales</h1>
      <p class="ayuda">
        Bóveda write-only: los valores jamás se muestran ni se descargan; solo
        se crean y se rotan. Cada ejecución audita qué versión usó.
      </p>
      {aviso && <p class="ayuda">{aviso}</p>}
      {error && <p class="error-global">{error}</p>}

      {administra && (
        <form class="tarjeta" onSubmit={crear}>
          <h3>nueva credencial</h3>
          <label for="cred-nombre">nombre</label>
          <input id="cred-nombre" value={nombre}
            onInput={(e) => setNombre((e.target as HTMLInputElement).value)} />
          {problemaDe("name") && <p class="error-campo">{problemaDe("name")}</p>}
          <label for="cred-secreto">secreto (se cifra en reposo)</label>
          <input id="cred-secreto" type="password" value={secreto}
            onInput={(e) => setSecreto((e.target as HTMLInputElement).value)} />
          {problemaDe("secret") && <p class="error-campo">{problemaDe("secret")}</p>}
          <label for="cred-desc">descripción</label>
          <input id="cred-desc" value={descripcion}
            onInput={(e) => setDescripcion((e.target as HTMLInputElement).value)} />
          <div class="acciones">
            <button type="submit">crear</button>
          </div>
        </form>
      )}

      <div class="tarjeta">
        <table>
          <thead>
            <tr>
              <th>nombre</th><th>descripción</th><th>versión vigente</th><th>alta</th>
              {administra && <th>acciones</th>}
            </tr>
          </thead>
          <tbody>
            {lista.map((credencial) => (
              <tr key={credencial.id}>
                <td>{credencial.name}</td>
                <td>{credencial.description}</td>
                <td>
                  {credencial.currentVersionId ? (
                    <span class="insignia">vigente</span>
                  ) : (
                    <span class="insignia">sin versión</span>
                  )}
                </td>
                <td>{new Date(credencial.createdAt).toLocaleDateString()}</td>
                {administra && (
                  <td>
                    <div class="acciones">
                      <input
                        type="password"
                        placeholder="valor nuevo"
                        style="max-width: 140px;"
                        value={rotando[credencial.id] ?? ""}
                        onInput={(e) =>
                          setRotando((previo) => ({
                            ...previo,
                            [credencial.id]: (e.target as HTMLInputElement).value,
                          }))
                        }
                      />
                      <button class="secundario" onClick={() => void rotar(credencial.id)}>
                        rotar
                      </button>
                      <button class="secundario" onClick={() => void abrirHistorial(credencial)}>
                        historial
                      </button>
                      <input
                        placeholder={`teclea «${credencial.name}»`}
                        style="max-width: 140px;"
                        value={borrando[credencial.id] ?? ""}
                        onInput={(e) =>
                          setBorrando((previo) => ({
                            ...previo,
                            [credencial.id]: (e.target as HTMLInputElement).value,
                          }))
                        }
                      />
                      <button
                        class="peligro"
                        disabled={
                          !confirmarEliminacion(credencial.name, borrando[credencial.id] ?? "")
                        }
                        onClick={() => void eliminar(credencial)}
                      >
                        eliminar
                      </button>
                    </div>
                  </td>
                )}
              </tr>
            ))}
            {lista.length === 0 && <tr><td colspan={5}>sin credenciales</td></tr>}
          </tbody>
        </table>

        {lista.map((credencial) =>
          historial[credencial.id] ? (
            <details key={`hist-${credencial.id}`} open>
              <summary>historial de {credencial.name}</summary>
              <table>
                <thead>
                  <tr><th>versión</th><th>creada</th><th>estado</th></tr>
                </thead>
                <tbody>
                  {(historial[credencial.id] ?? []).map((version) => (
                    <tr key={version.id}>
                      <td>v{version.version}</td>
                      <td>{new Date(version.createdAt).toLocaleString()}</td>
                      <td>
                        {esVigente(version.id, credencial.currentVersionId) ? (
                          <span class="insignia">vigente</span>
                        ) : (
                          "reemplazada"
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <p class="ayuda">ids y fechas solamente: los valores nunca salen de la bóveda.</p>
            </details>
          ) : null,
        )}

        {!administra && <p class="ayuda">solo un editor puede crear, rotar o eliminar credenciales</p>}
      </div>
    </div>
  );
}
