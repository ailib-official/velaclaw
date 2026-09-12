import { describe, expect, it } from "vitest";
import {
  parseMemoryEntry,
  parseMemoryList,
  parseSessionList,
  parseSessionSummary,
  parseWsDoneFrame,
} from "./contract";
import memoryList from "./fixtures/memory_list.json";
import sessionList from "./fixtures/session_list.json";
import wsDone from "./fixtures/ws_done.json";

describe("VL-UI-010 web/rust golden fixtures", () => {
  it("parses SessionSummary list from gateway JSON", () => {
    const sessions = parseSessionList(sessionList);
    expect(sessions).toHaveLength(1);
    const summary = parseSessionSummary(sessions[0]);
    expect(summary.id).toBe("sess-1");
    expect(summary.model_id).toBe("nvidia/nemotron-mini-4b-instruct");
    expect(summary.message_count).toBe(2);
  });

  it("fails if session message_count is removed", () => {
    const { message_count: _drop, ...broken } = sessionList.sessions[0];
    expect(() => parseSessionSummary(broken)).toThrow(/message_count/);
  });

  it("parses custom memory category as a string", () => {
    const { entries, total } = parseMemoryList(memoryList);
    expect(total).toBe(1);
    expect(entries[0]?.category).toBe("project_notes");
    expect(entries[0]?.session_id).toBe("sess-1");
  });

  it("rejects object-shaped Custom category", () => {
    expect(() =>
      parseMemoryEntry({
        id: "x",
        key: "k",
        content: "c",
        category: { Custom: "project_notes" },
        timestamp: "2026-09-12T00:00:00Z",
      }),
    ).toThrow(/JSON string/);
  });

  it("parses WS done observe metadata", () => {
    const frame = parseWsDoneFrame(wsDone);
    expect(frame.type).toBe("done");
    expect(frame.selected_model).toBe("nvidia/nemotron-mini-4b-instruct");
    expect(frame.model_selection_reason).toBe("explicit_user_pick");
    expect(frame.usage).toEqual({ input_tokens: 1, output_tokens: 2 });
    expect(frame.cost).toBe(0.001);
  });

  it("fails if selected_model is renamed away", () => {
    const { selected_model: _drop, ...broken } = wsDone;
    expect(() => parseWsDoneFrame(broken)).toThrow(/selected_model/);
  });
});
