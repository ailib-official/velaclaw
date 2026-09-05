import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  formatRelativeTime,
  formatSessionMeta,
  loadActiveSessionId,
  resolveInitialSessionId,
  saveActiveSessionId,
  applySessionTitle,
} from "./sessions";

function mockLocalStorage() {
  const store = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, value);
    },
    removeItem: (key: string) => {
      store.delete(key);
    },
    clear: () => {
      store.clear();
    },
  });
}

describe("session persistence helpers", () => {
  beforeEach(() => {
    mockLocalStorage();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("round-trips active session id in localStorage", () => {
    expect(loadActiveSessionId()).toBeNull();
    saveActiveSessionId("sess-abc");
    expect(loadActiveSessionId()).toBe("sess-abc");
    saveActiveSessionId(null);
    expect(loadActiveSessionId()).toBeNull();
  });

  it("prefers URL session param over localStorage", () => {
    saveActiveSessionId("stored-id");
    expect(resolveInitialSessionId("?session=url-id")).toBe("url-id");
  });

  it("falls back to localStorage when URL param absent", () => {
    saveActiveSessionId("stored-id");
    expect(resolveInitialSessionId("")).toBe("stored-id");
  });
});

describe("formatRelativeTime", () => {
  it("formats recent timestamps", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-30T12:00:00Z"));
    expect(formatRelativeTime("2026-07-30T11:30:00Z")).toBe("30m ago");
    vi.useRealTimers();
  });
});

describe("formatSessionMeta", () => {
  it("includes message count, relative time, and model", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-30T12:00:00Z"));
    const meta = formatSessionMeta({
      id: "s1",
      title: "Test",
      created_at: "2026-07-30T10:00:00Z",
      updated_at: "2026-07-30T11:00:00Z",
      model_id: "openai/gpt-4",
      message_count: 4,
    });
    expect(meta).toContain("4 msgs");
    expect(meta).toContain("1h ago");
    expect(meta).toContain("openai/gpt-4");
    vi.useRealTimers();
  });

  it("applies a pushed title to the matching session", () => {
    const next = applySessionTitle(
      [
        {
          id: "s1",
          title: "New session",
          created_at: "2026-07-30T10:00:00Z",
          updated_at: "2026-07-30T11:00:00Z",
          message_count: 3,
        },
      ],
      "s1",
      "LAN Scan and Xray Proxy Check",
    );
    expect(next[0]?.title).toBe("LAN Scan and Xray Proxy Check");
  });
});
