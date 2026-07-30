import { describe, expect, it } from "vitest";
import {
  DOCTOR_CAP_ROUTE_COMMANDS,
  formatRoutingSummary,
  routingDiagnosticsFromConfig,
} from "./diagnostics";

describe("routingDiagnosticsFromConfig", () => {
  it("hides panel by default", () => {
    const view = routingDiagnosticsFromConfig({
      default_model: "openai/gpt-4o-mini",
      routing: { provider_mode: "byok" },
      agent: { intent_capability_route: false },
    });
    expect(view.panelVisible).toBe(false);
    expect(view.modelRouteCount).toBe(0);
  });

  it("shows panel when intent_capability_route is enabled", () => {
    const view = routingDiagnosticsFromConfig({
      default_model: "deepseek/deepseek-chat",
      routing: { provider_mode: "prism" },
      agent: { intent_capability_route: true },
      model_routes: [{ hint: "coding" }, { hint: "fast" }],
    });
    expect(view.panelVisible).toBe(true);
    expect(view.providerMode).toBe("prism");
    expect(view.modelRouteCount).toBe(2);
    expect(view.doctorCommands).toEqual(DOCTOR_CAP_ROUTE_COMMANDS);
  });
});

describe("formatRoutingSummary", () => {
  it("renders non-secret routing lines", () => {
    const summary = formatRoutingSummary({
      panelVisible: true,
      providerMode: "byok",
      defaultModel: "openai/gpt-4o-mini",
      intentCapabilityRoute: true,
      modelRouteCount: 1,
      doctorCommands: DOCTOR_CAP_ROUTE_COMMANDS,
    });
    expect(summary).toContain("provider_mode: byok");
    expect(summary).toContain("intent_capability_route: true");
  });
});
