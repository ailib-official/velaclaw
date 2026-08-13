import { describe, expect, it } from "vitest";
import {
  appendAssistantDelta,
  appendSystemNotice,
  applyStatusFrame,
  applyStepFrame,
  lastAssistantHasVelaClawNotice,
  looksLikeVelaClawNotice,
  outboundChatHistory,
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

describe("progress frames", () => {
  it("replaces last status line", () => {
    const a = applyStatusFrame([{ role: "user", content: "hi" }], "model", "deepseek/x");
    const b = applyStatusFrame(a, "tool", "shell");
    expect(b.filter((m) => m.role === "status")).toHaveLength(1);
    expect(b[b.length - 1].content).toBe("tool: shell");
  });

  it("appends step with ok flag", () => {
    const next = applyStepFrame([{ role: "user", content: "hi" }], {
      tool: "shell",
      ok: true,
      summary: "ls",
    });
    expect(next[1]).toEqual({ role: "step", content: "shell: ls", stepOk: true });
  });

  it("outbound history drops status and step", () => {
    const hist = outboundChatHistory([
      { role: "user", content: "hi" },
      { role: "status", content: "model: x" },
      { role: "step", content: "shell: ok", stepOk: true },
      { role: "assistant", content: "done" },
    ]);
    expect(hist.map((m) => m.role)).toEqual(["user", "assistant"]);
  });
});

describe("progress frames", () => {
  it("replaces last status line", () => {
    const a = applyStatusFrame([{ role: "user", content: "hi" }], "model", "deepseek/x");
    const b = applyStatusFrame(a, "tool", "shell");
    expect(b.filter((m) => m.role === "status")).toHaveLength(1);
    expect(b[b.length - 1].content).toBe("tool: shell");
  });

  it("appends step with ok flag", () => {
    const next = applyStepFrame([{ role: "user", content: "hi" }], {
      tool: "shell",
      ok: true,
      summary: "ls",
    });
    expect(next[1]).toEqual({ role: "step", content: "shell: ls", stepOk: true });
  });

  it("outbound history drops status and step", () => {
    const hist = outboundChatHistory([
      { role: "user", content: "hi" },
      { role: "status", content: "model: x" },
      { role: "step", content: "shell: ok", stepOk: true },
      { role: "assistant", content: "done" },
    ]);
    expect(hist.map((m) => m.role)).toEqual(["user", "assistant"]);
  });
});
