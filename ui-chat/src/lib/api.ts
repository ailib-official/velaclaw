const TOKEN_KEY = "velaclaw_bearer_token";

export function loadToken(): string {
  try {
    return localStorage.getItem(TOKEN_KEY) ?? "";
  } catch {
    return "";
  }
}

export function saveToken(token: string): void {
  try {
    localStorage.setItem(TOKEN_KEY, token.trim());
  } catch {
    /* ignore */
  }
}

export interface ProviderModel {
  logical_id: string;
  provider: string;
  available: boolean;
}

export interface ProvidersResponse {
  providers: { id: string; available: boolean; required_envs: string[] }[];
  models: ProviderModel[];
}

export async function fetchProviders(token: string): Promise<ProvidersResponse> {
  const headers: Record<string, string> = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetch("/api/providers", { headers });
  if (!res.ok) {
    throw new Error(`providers ${res.status}: ${await res.text()}`);
  }
  return res.json();
}

export async function fetchHealth(): Promise<{ paired?: boolean; status?: string }> {
  const res = await fetch("/health");
  if (!res.ok) throw new Error(`health ${res.status}`);
  return res.json();
}

function authHeaders(token: string): Record<string, string> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (token) headers.Authorization = `Bearer ${token}`;
  return headers;
}

export interface SessionSummary {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  model_id?: string;
  message_count: number;
}

export interface SessionDetail extends SessionSummary {
  messages: { role: string; content: string }[];
}

export async function fetchSessions(token: string): Promise<SessionSummary[]> {
  const res = await fetch("/api/sessions", { headers: authHeaders(token) });
  if (!res.ok) throw new Error(`sessions ${res.status}: ${await res.text()}`);
  const data = await res.json();
  return data.sessions ?? [];
}

export async function createSession(
  token: string,
  title?: string,
  modelId?: string,
): Promise<SessionDetail> {
  const res = await fetch("/api/sessions", {
    method: "POST",
    headers: authHeaders(token),
    body: JSON.stringify({ title, model_id: modelId }),
  });
  if (!res.ok) throw new Error(`create session ${res.status}: ${await res.text()}`);
  return res.json();
}

export async function fetchSession(token: string, id: string): Promise<SessionDetail> {
  const res = await fetch(`/api/sessions/${encodeURIComponent(id)}`, {
    headers: authHeaders(token),
  });
  if (!res.ok) throw new Error(`session ${res.status}: ${await res.text()}`);
  return res.json();
}

export async function deleteSession(token: string, id: string): Promise<void> {
  const res = await fetch(`/api/sessions/${encodeURIComponent(id)}`, {
    method: "DELETE",
    headers: authHeaders(token),
  });
  if (!res.ok) throw new Error(`delete session ${res.status}: ${await res.text()}`);
}

export interface MemoryEntry {
  id: string;
  key: string;
  content: string;
  category: string;
  timestamp: string;
}

export async function fetchMemory(
  token: string,
  q?: string,
): Promise<{ entries: MemoryEntry[]; total: number }> {
  const params = new URLSearchParams();
  if (q) params.set("q", q);
  const res = await fetch(`/api/memory?${params}`, { headers: authHeaders(token) });
  if (!res.ok) throw new Error(`memory ${res.status}: ${await res.text()}`);
  return res.json();
}

export async function fetchConfig(token: string): Promise<Record<string, unknown>> {
  const res = await fetch("/api/config", { headers: authHeaders(token) });
  if (!res.ok) throw new Error(`config ${res.status}: ${await res.text()}`);
  return res.json();
}

export async function fetchConfigSchema(token: string): Promise<unknown> {
  const res = await fetch("/api/config/schema", { headers: authHeaders(token) });
  if (!res.ok) throw new Error(`config schema ${res.status}: ${await res.text()}`);
  return res.json();
}

export async function putConfig(
  token: string,
  patch: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  const res = await fetch("/api/config", {
    method: "PUT",
    headers: authHeaders(token),
    body: JSON.stringify(patch),
  });
  if (!res.ok) throw new Error(`config put ${res.status}: ${await res.text()}`);
  return res.json();
}
