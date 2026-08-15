export type ChatRole = "user" | "assistant" | "system" | "status" | "step";

export interface ChatMessage {
  role: ChatRole;
  content: string;
  stepOk?: boolean;
  /** Scrubbed tool output; shown only when the user expands a step. */
  expand?: string;
}

/** Marker emitted by ORCH-HOST-004 soft-fail / quota notices (do not re-classify in UI). */
export const VELACLAW_NOTICE_MARKER = "VelaClaw notice:";

export function looksLikeVelaClawNotice(text: string): boolean {
  return text.includes(VELACLAW_NOTICE_MARKER);
}

export function appendSystemNotice(messages: ChatMessage[], content: string): ChatMessage[] {
  return [...messages, { role: "system", content }];
}

/** Last assistant blob contains a soft-fail / failover notice (streamed in reply text). */
export function lastAssistantHasVelaClawNotice(messages: ChatMessage[]): boolean {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const m = messages[i];
    if (m.role === "assistant") {
      return looksLikeVelaClawNotice(m.content);
    }
  }
  return false;
}

export interface WsServerFrame {
  type:
    | "delta"
    | "done"
    | "error"
    | "approval_required"
    | "input_required"
    | "status"
    | "step"
    | "cancelled";
  content?: string;
  message?: string;
  usage?: { input_tokens: number; output_tokens: number };
  cost?: number;
  id?: string;
  tool_name?: string;
  arguments_summary?: string;
  kind?: string;
  prompt?: string;
  options?: string[];
  risk_note?: string;
  phase?: string;
  detail?: string;
  tool?: string;
  ok?: boolean;
  summary?: string;
  expand?: string;
}

export interface ApprovalRequiredPayload {
  id: string;
  tool_name: string;
  arguments_summary: string;
}

export interface HumanInputRequiredPayload {
  id: string;
  kind: "choice" | "text" | "secret" | "handoff" | string;
  prompt: string;
  options: string[];
  risk_note?: string;
}

export interface StreamChatOptions {
  token: string;
  messages: ChatMessage[];
  sessionId?: string;
  modelId?: string;
  temperature?: number;
  onDelta: (chunk: string) => void;
  onDone: (frame: WsServerFrame) => void;
  onError: (message: string) => void;
  onApprovalRequired?: (payload: ApprovalRequiredPayload) => void;
  onInputRequired?: (payload: HumanInputRequiredPayload) => void;
  onStatus?: (phase: string, detail?: string) => void;
  onStep?: (payload: {
    kind: string;
    tool: string;
    ok: boolean;
    summary: string;
    expand?: string;
  }) => void;
  onCancelled?: (message?: string) => void;
}

function wsUrl(token: string): string {
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  const host = window.location.host;
  const q = token ? `?token=${encodeURIComponent(token)}` : "";
  return `${proto}//${host}/ws${q}`;
}

export function streamChat(opts: StreamChatOptions): () => void {
  const socket = new WebSocket(wsUrl(opts.token));
  let closed = false;

  socket.addEventListener("open", () => {
    socket.send(
      JSON.stringify({
        type: "chat",
        session_id: opts.sessionId,
        messages: opts.messages,
        model_id: opts.modelId,
        temperature: opts.temperature,
      }),
    );
  });

  socket.addEventListener("message", (ev) => {
    let frame: WsServerFrame;
    try {
      frame = JSON.parse(String(ev.data));
    } catch {
      opts.onError("Invalid WebSocket JSON");
      socket.close();
      return;
    }
    if (frame.type === "delta" && frame.content) {
      opts.onDelta(frame.content);
    } else if (frame.type === "status" && frame.phase) {
      opts.onStatus?.(frame.phase, frame.detail);
    } else if (frame.type === "step" && frame.tool) {
      opts.onStep?.({
        kind: frame.kind ?? "tool_result",
        tool: frame.tool,
        ok: frame.ok !== false,
        summary: frame.summary ?? "",
        expand: frame.expand,
      });
    } else if (frame.type === "cancelled") {
      opts.onCancelled?.(frame.message);
      socket.close();
    } else if (frame.type === "approval_required" && frame.id && frame.tool_name) {
      opts.onApprovalRequired?.({
        id: frame.id,
        tool_name: frame.tool_name,
        arguments_summary: frame.arguments_summary ?? "",
      });
    } else if (frame.type === "input_required" && frame.id && frame.prompt) {
      opts.onInputRequired?.({
        id: frame.id,
        kind: frame.kind ?? "text",
        prompt: frame.prompt,
        options: frame.options ?? [],
        risk_note: frame.risk_note,
      });
    } else if (frame.type === "done") {
      opts.onDone(frame);
      socket.close();
    } else if (frame.type === "error") {
      opts.onError(frame.message ?? "Unknown error");
      socket.close();
    }
  });

  socket.addEventListener("error", () => {
    if (!closed) opts.onError("WebSocket connection failed");
  });

  socket.addEventListener("close", () => {
    closed = true;
  });

  return () => {
    closed = true;
    if (socket.readyState === WebSocket.OPEN) {
      try {
        socket.send(JSON.stringify({ type: "cancel" }));
      } catch {
        /* ignore */
      }
    }
    socket.close();
  };
}

/** Pure reducer for tests — append streaming delta to assistant message. */
export function appendAssistantDelta(messages: ChatMessage[], delta: string): ChatMessage[] {
  const out = [...messages];
  const last = out[out.length - 1];
  if (last?.role === "assistant") {
    out[out.length - 1] = { role: "assistant", content: last.content + delta };
  } else {
    out.push({ role: "assistant", content: delta });
  }
  return out;
}

/** History sent to the model: user + assistant only. */
export function outboundChatHistory(messages: ChatMessage[]): ChatMessage[] {
  return messages.filter((m) => m.role === "user" || m.role === "assistant");
}

export function applyStatusFrame(messages: ChatMessage[], phase: string, detail?: string): ChatMessage[] {
  const content = detail && detail.length > 0 ? detail : phase;
  const out = [...messages];
  const last = out[out.length - 1];
  if (last?.role === "status") {
    out[out.length - 1] = { role: "status", content };
  } else {
    out.push({ role: "status", content });
  }
  return out;
}

export function applyStepFrame(
  messages: ChatMessage[],
  payload: { tool: string; ok: boolean; summary: string; expand?: string },
): ChatMessage[] {
  const content = payload.summary || payload.tool;
  const msg: ChatMessage = { role: "step", content, stepOk: payload.ok };
  if (payload.expand) {
    msg.expand = payload.expand;
  }
  const out = [...messages];
  // In-flight "run …" status is the same step; do not keep both lines.
  if (out[out.length - 1]?.role === "status") {
    out.pop();
  }
  out.push(msg);
  return out;
}
