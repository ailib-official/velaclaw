import type { MemoryEntry, SessionSummary } from "./api";
import type { WsServerFrame } from "./chat";

function requireString(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${field} must be a non-empty string`);
  }
  return value;
}

function optionalString(value: unknown, field: string): string | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== "string") {
    throw new Error(`${field} must be a string when present`);
  }
  return value;
}

function requireNumber(value: unknown, field: string): number {
  if (typeof value !== "number" || Number.isNaN(value)) {
    throw new Error(`${field} must be a number`);
  }
  return value;
}

/** Parse GET /api/sessions item. Fails if a required field is renamed or missing. */
export function parseSessionSummary(raw: unknown): SessionSummary {
  if (typeof raw !== "object" || raw === null) {
    throw new Error("SessionSummary must be an object");
  }
  const o = raw as Record<string, unknown>;
  return {
    id: requireString(o.id, "id"),
    title: requireString(o.title, "title"),
    created_at: requireString(o.created_at, "created_at"),
    updated_at: requireString(o.updated_at, "updated_at"),
    model_id: optionalString(o.model_id, "model_id"),
    message_count: requireNumber(o.message_count, "message_count"),
  };
}

export function parseSessionList(raw: unknown): SessionSummary[] {
  if (typeof raw !== "object" || raw === null) {
    throw new Error("session list must be an object");
  }
  const sessions = (raw as Record<string, unknown>).sessions;
  if (!Array.isArray(sessions)) {
    throw new Error("sessions must be an array");
  }
  return sessions.map(parseSessionSummary);
}

/**
 * Parse a memory API entry. `category` must be a JSON string (core/daily/conversation
 * or custom Display name). Object-shaped serde Custom variants are rejected.
 */
export function parseMemoryEntry(raw: unknown): MemoryEntry {
  if (typeof raw !== "object" || raw === null) {
    throw new Error("MemoryEntry must be an object");
  }
  const o = raw as Record<string, unknown>;
  if (typeof o.category !== "string") {
    throw new Error("category must be a JSON string, not an enum object");
  }
  return {
    id: requireString(o.id, "id"),
    key: requireString(o.key, "key"),
    content: requireString(o.content, "content"),
    category: o.category,
    timestamp: requireString(o.timestamp, "timestamp"),
    session_id: optionalString(o.session_id, "session_id"),
    score: o.score === undefined || o.score === null ? undefined : requireNumber(o.score, "score"),
  };
}

export function parseMemoryList(raw: unknown): { entries: MemoryEntry[]; total: number } {
  if (typeof raw !== "object" || raw === null) {
    throw new Error("memory list must be an object");
  }
  const o = raw as Record<string, unknown>;
  if (!Array.isArray(o.entries)) {
    throw new Error("entries must be an array");
  }
  return {
    entries: o.entries.map(parseMemoryEntry),
    total: requireNumber(o.total, "total"),
  };
}

/** Parse a WebSocket `done` frame including observe metadata. */
export function parseWsDoneFrame(raw: unknown): WsServerFrame {
  if (typeof raw !== "object" || raw === null) {
    throw new Error("WS frame must be an object");
  }
  const o = raw as Record<string, unknown>;
  if (o.type !== "done") {
    throw new Error(`expected type=done, got ${String(o.type)}`);
  }
  const frame: WsServerFrame = { type: "done" };
  if (o.usage !== undefined && o.usage !== null) {
    if (typeof o.usage !== "object") {
      throw new Error("usage must be an object");
    }
    const usage = o.usage as Record<string, unknown>;
    frame.usage = {
      input_tokens: requireNumber(usage.input_tokens, "usage.input_tokens"),
      output_tokens: requireNumber(usage.output_tokens, "usage.output_tokens"),
    };
  }
  if (o.cost !== undefined && o.cost !== null) {
    frame.cost = requireNumber(o.cost, "cost");
  }
  frame.selected_model = optionalString(o.selected_model, "selected_model");
  frame.model_selection_reason = optionalString(
    o.model_selection_reason,
    "model_selection_reason",
  );
  if (!frame.selected_model) {
    throw new Error("done frame fixture must include selected_model");
  }
  if (!frame.model_selection_reason) {
    throw new Error("done frame fixture must include model_selection_reason");
  }
  return frame;
}
