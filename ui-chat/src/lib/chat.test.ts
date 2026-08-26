import { describe, expect, it } from "vitest";
    import {
    appendAssistantDelta,
    appendSystemNotice,
    applyStatusFrame,
    applyStepFrame,
    chatClientFrame,
    clearStatusMessages,
    lastAssistantHasVelaClawNotice,
    looksLikeVelaClawNotice,
    outboundChatHistory,
    parseLiveDagPreview,
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

  it("drops trailing status when assistant reply starts", () => {
    const next = appendAssistantDelta(
      [
        { role: "user", content: "hi" },
        { role: "status", content: "nvidia/nemotron-x" },
      ],
      "Hello",
    );
    expect(next).toEqual([
      { role: "user", content: "hi" },
      { role: "assistant", content: "Hello" },
    ]);
  });
});

describe("clearStatusMessages", () => {
  it("removes status roles only", () => {
    const next = clearStatusMessages([
      { role: "user", content: "hi" },
      { role: "status", content: "model/x" },
      { role: "assistant", content: "ok" },
    ]);
    expect(next).toEqual([
      { role: "user", content: "hi" },
      { role: "assistant", content: "ok" },
    ]);
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
  it("replaces last status line with caption text", () => {
    const a = applyStatusFrame([{ role: "user", content: "hi" }], "model", "deepseek/x");
    const b = applyStatusFrame(a, "run", "git status");
    expect(b.filter((m) => m.role === "status")).toHaveLength(1);
    expect(b[b.length - 1].content).toBe("git status");
  });

  it("appends step with ok flag using caption only", () => {
    const next = applyStepFrame([{ role: "user", content: "hi" }], {
      tool: "shell",
      ok: true,
      summary: "git status",
      expand: "On branch main",
    });
    expect(next[1]).toEqual({
      role: "step",
      content: "git status",
      stepOk: true,
      expand: "On branch main",
    });
    expect(next[1].content).not.toContain("On branch main");
  });

  it("step replaces trailing in-flight status", () => {
    const withStatus = applyStatusFrame([{ role: "user", content: "hi" }], "run", "ls workspace");
    const next = applyStepFrame(withStatus, {
      tool: "shell",
      ok: true,
      summary: "ls workspace",
      expand: "file.txt\n",
    });
    expect(next.map((m) => m.role)).toEqual(["user", "step"]);
    expect(next[1].expand).toBe("file.txt\n");
  });

  it("outbound history drops status and step", () => {
    const hist = outboundChatHistory([
      { role: "user", content: "hi" },
      { role: "status", content: "deepseek/x" },
      { role: "step", content: "git status", stepOk: true },
      { role: "assistant", content: "done" },
    ]);
    expect(hist.map((m) => m.role)).toEqual(["user", "assistant"]);
  });
});

describe("chatClientFrame", () => {
  it("defaults host_phase to build", () => {
    const frame = chatClientFrame({
      messages: [{ role: "user", content: "hi" }],
    });
    expect(frame.host_phase).toBe("build");
    expect(frame.type).toBe("chat");
  });

  it("sends plan when requested", () => {
    const frame = chatClientFrame({
      messages: [{ role: "user", content: "hi" }],
      hostPhase: "plan",
      sessionId: "s1",
    });
    expect(frame.host_phase).toBe("plan");
  });
});

describe("parseLiveDagPreview", () => {
  it("parses numbered linear nodes", () => {
    const text = `Planner accepted linear DAG \`paper-slides\`.

Bounded task DAG \`paper-slides\` (2 node(s), max_steps=8). Approve Build to run each node through the existing tool loop.

1. read  task_type=summarize  caps=document_understanding  next=slides
2. slides  task_type=write  caps=speed  next=(end)
`;
    const parsed = parseLiveDagPreview(text);
    expect(parsed?.dagId).toBe("paper-slides");
    expect(parsed?.fallback).toBe(false);
    expect(parsed?.nodes.map((n) => n.id)).toEqual(["read", "slides"]);
  });

  it("marks handwritten fallback", () => {
    const text = `Planner output was not a valid linear L2 DAG; using handwritten fallback.

Bounded task DAG \`code-fix-template\` (3 node(s), max_steps=8). Approve Build to run each node through the existing tool loop.

1. locate  task_type=code-fix  caps=coding  next=patch
`;
    expect(parseLiveDagPreview(text)?.fallback).toBe(true);
  });
});
