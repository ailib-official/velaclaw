import { describe, expect, it } from "vitest";
import { appendAssistantDelta } from "./chat";

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
