//! Cliente del plano de administración: la consola NO añade endpoints
//! nuevos; todo va por `/api/v1` con el Bearer de la API key del workspace.

/// Clave de la API key en el almacenamiento del navegador.
const KEY_STORAGE = "integraciones.apiKey";

export function getKey(): string | null {
  return localStorage.getItem(KEY_STORAGE);
}

export function setKey(key: string): void {
  localStorage.setItem(KEY_STORAGE, key);
}

export function clearKey(): void {
  localStorage.removeItem(KEY_STORAGE);
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
  const key = getKey();
  const respuesta = await fetch(`/api/v1${path}`, {
    method,
    headers: {
      ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
      ...(key ? { Authorization: `Bearer ${key}` } : {}),
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

/// Página del listado paginado por cursor (contrato `Page` del API).
export interface Page<T> {
  items: T[];
  nextCursor: string | null;
}
