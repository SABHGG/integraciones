//! Cliente del plano de administración: la consola NO añade endpoints
//! nuevos; todo va por `/api/v1`. MVP4: la sesión es un token de usuario
//! (login email+contraseña) o la API key de workspace (acceso alternativo
//! de automatización); ambas viajan como Bearer.

/// Claves en el almacenamiento del navegador.
const TOKEN_STORAGE = "integraciones.token";
const KEY_STORAGE = "integraciones.apiKey";

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_STORAGE);
}

export function setToken(token: string): void {
  localStorage.setItem(TOKEN_STORAGE, token);
  localStorage.removeItem(KEY_STORAGE);
}

export function getApiKey(): string | null {
  return localStorage.getItem(KEY_STORAGE);
}

/** Acceso alternativo de automatización/demo: la key actúa como owner. */
export function setApiKey(key: string): void {
  localStorage.setItem(KEY_STORAGE, key);
  localStorage.removeItem(TOKEN_STORAGE);
}

export function clearSesion(): void {
  localStorage.removeItem(TOKEN_STORAGE);
  localStorage.removeItem(KEY_STORAGE);
}

/** Modo de la sesión activa (para whoami y la UI por rol). */
export function modoSesion(): "usuario" | "api_key" | null {
  if (getToken()) return "usuario";
  if (getApiKey()) return "api_key";
  return null;
}

/// Error del plano admin con los issues de validación si los hay (422).
export class ApiError extends Error {
  status: number;
  title: string;
  /** Issues del validador: `{field, message}` (422), vacío en otros casos. */
  fields: { field: string; message: string }[];

  constructor(status: number, title: string, fields: { field: string; message: string }[]) {
    super(title);
    this.status = status;
    this.title = title;
    this.fields = fields;
  }
}

interface Problema {
  title?: string;
  detail?: string;
  errors?: { field: string; message: string }[];
}

/// Petición al plano admin; lanza `ApiError` con el problema RFC 9457.
export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  const bearer = getToken() ?? getApiKey();
  const respuesta = await fetch(`/api/v1${path}`, {
    method,
    headers: {
      ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
      ...(bearer ? { Authorization: `Bearer ${bearer}` } : {}),
    },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (respuesta.ok) {
    if (respuesta.status === 204) return undefined as T;
    return (await respuesta.json()) as T;
  }
  let problema: Problema = {};
  try {
    problema = (await respuesta.json()) as Problema;
  } catch {
    // cuerpo no JSON: nos quedamos con el status.
  }
  throw new ApiError(
    respuesta.status,
    problema.detail ?? problema.title ?? `error ${respuesta.status}`,
    problema.errors ?? [],
  );
}

/// Respuesta del login: token + workspace activo (si quedó fijado) + la
/// membresía completa del usuario (para elegir workspace).
export interface SesionLogin {
  token: string;
  expiresAt: string;
  activeWorkspace: { id: string; slug: string; name: string; role: string } | null;
  workspaces: { id: string; slug: string; name: string; role: string }[];
}

/// Registro público: crear cuenta no da acceso a ningún workspace.
export async function registrar(email: string, password: string): Promise<SesionLogin> {
  return api<SesionLogin>("POST", "/auth/register", { email, password });
}

/// Login con email+contraseña; `workspace` (slug) fija el workspace de la
/// sesión cuando el usuario pertenece a varios.
export async function entrar(
  email: string,
  password: string,
  workspace?: string,
): Promise<SesionLogin> {
  const sesion = await api<SesionLogin>("POST", "/auth/login", {
    email,
    password,
    ...(workspace ? { workspace } : {}),
  });
  setToken(sesion.token);
  return sesion;
}

/// Logout: revoca la sesión EN EL SERVIDOR y borra el token local.
export async function salir(): Promise<void> {
  try {
    if (getToken()) {
      await api("POST", "/auth/logout");
    }
  } finally {
    clearSesion();
  }
}

/// Página del listado paginado por cursor (contrato `Page` del API).
export interface Page<T> {
  items: T[];
  nextCursor: string | null;
}
