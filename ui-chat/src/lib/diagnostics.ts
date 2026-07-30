/** Host capability / routing diagnostics for Settings (VL-UI-009). */

export const DOCTOR_CAP_ROUTE_COMMANDS = [
  "velaclaw doctor capabilities --tag <Tag>",
  "velaclaw doctor capability-route --tag <Tag> --force",
  "velaclaw doctor routing",
] as const;

export interface RoutingDiagnosticsView {
  panelVisible: boolean;
  providerMode: string | null;
  defaultModel: string | null;
  intentCapabilityRoute: boolean;
  modelRouteCount: number;
  doctorCommands: readonly string[];
}

export function routingDiagnosticsFromConfig(
  config: Record<string, unknown>,
): RoutingDiagnosticsView {
  const agent = config.agent as Record<string, unknown> | undefined;
  const routing = config.routing as Record<string, unknown> | undefined;
  const modelRoutes = config.model_routes;
  const intentCapabilityRoute = agent?.intent_capability_route === true;

  return {
    panelVisible: intentCapabilityRoute,
    providerMode: routing?.provider_mode != null ? String(routing.provider_mode) : null,
    defaultModel: config.default_model != null ? String(config.default_model) : null,
    intentCapabilityRoute,
    modelRouteCount: Array.isArray(modelRoutes) ? modelRoutes.length : 0,
    doctorCommands: DOCTOR_CAP_ROUTE_COMMANDS,
  };
}

export function formatRoutingSummary(view: RoutingDiagnosticsView): string {
  return [
    `provider_mode: ${view.providerMode ?? "byok"}`,
    `default_model: ${view.defaultModel ?? "(unset)"}`,
    `intent_capability_route: ${view.intentCapabilityRoute}`,
    `model_routes: ${view.modelRouteCount} rule(s)`,
  ].join("\n");
}
