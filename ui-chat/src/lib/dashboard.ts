/** Dashboard API payload from GET /api/dashboard (VL-UI-006). */

export interface DashboardCostSummary {
  session_cost_usd?: number;
  daily_cost_usd?: number;
  monthly_cost_usd?: number;
  total_tokens?: number;
  request_count?: number;
  by_model?: Record<string, unknown>;
}

export interface DashboardHealth {
  status?: string;
  paired?: boolean;
  runtime?: Record<string, unknown>;
  execution?: {
    runtime_kind?: string;
    docker_active?: boolean;
    sandbox?: string;
    sandbox_source?: string;
    escape_on_approval?: boolean;
    autonomy_level?: string;
    envelope_assemble?: boolean;
    host_decide?: boolean;
    intent_capability_route?: boolean;
    note?: string;
  };
}

export interface DashboardPayload {
  health: DashboardHealth;
  cost?: DashboardCostSummary | null;
}

/** View model for Overview tab rendering. */
export interface DashboardView {
  status: string;
  paired: boolean | null;
  hasCost: boolean;
  sessionCostUsd: number | null;
  dailyCostUsd: number | null;
  monthlyCostUsd: number | null;
  totalTokens: number | null;
  requestCount: number | null;
  runtimeJson: string;
  executionSummary: string | null;
}

export function dashboardViewFromPayload(payload: DashboardPayload): DashboardView {
  const health = payload.health ?? {};
  const cost = payload.cost ?? undefined;
  const hasCost = cost != null && cost.daily_cost_usd !== undefined;
  const exec = health.execution;
  const executionSummary = exec
    ? [
        `kind=${exec.runtime_kind ?? "unknown"}`,
        `docker_active=${exec.docker_active === true ? "yes" : "no"}`,
        `sandbox=${exec.sandbox ?? "unknown"}`,
        `escape_on_approval=${exec.escape_on_approval === true ? "yes" : "no"}`,
        `autonomy=${exec.autonomy_level ?? "unknown"}`,
        `envelope=${exec.envelope_assemble === true ? "on" : "off"}`,
        `host_decide=${exec.host_decide === true ? "on" : "off"}`,
        `capability_route=${exec.intent_capability_route === true ? "on" : "off"}`,
        exec.note ? String(exec.note) : "",
      ]
        .filter(Boolean)
        .join(" · ")
    : null;

  return {
    status: health.status ?? "unknown",
    paired: health.paired ?? null,
    hasCost,
    sessionCostUsd: cost?.session_cost_usd ?? null,
    dailyCostUsd: cost?.daily_cost_usd ?? null,
    monthlyCostUsd: cost?.monthly_cost_usd ?? null,
    totalTokens: cost?.total_tokens ?? null,
    requestCount: cost?.request_count ?? null,
    runtimeJson: JSON.stringify(health.runtime ?? {}, null, 2),
    executionSummary,
  };
}

export function formatUsd(value: number | null): string {
  if (value == null || Number.isNaN(value)) return "—";
  return `$${value.toFixed(4)}`;
}

export function formatInt(value: number | null): string {
  if (value == null || Number.isNaN(value)) return "—";
  return value.toLocaleString();
}
