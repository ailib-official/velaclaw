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
}

export function dashboardViewFromPayload(payload: DashboardPayload): DashboardView {
  const health = payload.health ?? {};
  const cost = payload.cost ?? undefined;
  const hasCost = cost != null && cost.daily_cost_usd !== undefined;

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
