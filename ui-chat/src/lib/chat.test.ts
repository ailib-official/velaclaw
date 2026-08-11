import { describe, expect, it } from "vitest";
import {
  appendAssistantDelta,
  appendSystemNotice,
  lastAssistantHasVelaClawNotice,
  looksLikeVelaClawNotice,
} from "./chat";

describe("appendAssistantDelta", () => {
  it("creates assistant message when none exists", () => {
    const next = appendAssistantDelta([{ role: "user", content: "hi" }], "Hello");
    expect(next).toEqual([
      { role: "user", content: "hi" },
      { role: "assistant", content: "Hello" },
    ]);
  });

  it("appends to existing assistant message", () => {
    const next = appendAssistantDelta(
      [
        { role: "user", content: "hi" },
        { role: "assistant", content: "Hel" },
      ],
      "lo",
    );
    expect(next[1].content).toBe("Hello");
  });
});

describe("velaClaw notice helpers", () => {
  it("detects notice marker", () => {
    expect(looksLikeVelaClawNotice("VelaClaw notice: tool-format")).toBe(true);
    expect(looksLikeVelaClawNotice("all good")).toBe(false);
  });

  it("appends system notice without mutating prior roles", () => {
    const next = appendSystemNotice([{ role: "user", content: "hi" }], "VelaClaw notice: quota");
    expect(next).toHaveLength(2);
    expect(next[1]).toEqual({ role: "system", content: "VelaClaw notice: quota" });
  });

  it("finds notice on last assistant message", () => {
    expect(
      lastAssistantHasVelaClawNotice([
        { role: "user", content: "x" },
        { role: "assistant", content: "ok\n\n---\nVelaClaw notice: exhausted" },
      ]),
    ).toBe(true);
    expect(
      lastAssistantHasVelaClawNotice([
        { role: "user", content: "x" },
        { role: "assistant", content: "ok" },
        { role: "system", content: "VelaClaw notice: other" },
      ]),
    ).toBe(false);
  });
});
