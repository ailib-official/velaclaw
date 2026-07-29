import { describe, expect, it } from "vitest";
import { dashboardViewFromPayload, formatUsd } from "./dashboard";

describe("dashboardViewFromPayload", () => {
  it("maps health and cost fields", () => {
    const view = dashboardViewFromPayload({
      health: {
        status: "ok",
        paired: true,
        runtime: { uptime_secs: 42 },
      },
      cost: {
        session_cost_usd: 0.0012,
        daily_cost_usd: 0.05,
        monthly_cost_usd: 1.25,
        total_tokens: 12000,
        request_count: 7,
      },
    });
    expect(view.status).toBe("ok");
    expect(view.paired).toBe(true);
    expect(view.hasCost).toBe(true);
    expect(view.dailyCostUsd).toBe(0.05);
    expect(view.runtimeJson).toContain("uptime_secs");
  });

  it("handles missing cost tracker", () => {
    const view = dashboardViewFromPayload({
      health: { status: "ok", paired: false },
      cost: null,
    });
    expect(view.hasCost).toBe(false);
    expect(view.dailyCostUsd).toBeNull();
  });
});

describe("formatUsd", () => {
  it("formats fixed decimals", () => {
    expect(formatUsd(0.05)).toBe("$0.0500");
    expect(formatUsd(null)).toBe("—");
  });
});
