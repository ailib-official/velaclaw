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
