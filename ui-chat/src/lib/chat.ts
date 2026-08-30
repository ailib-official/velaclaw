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
    | "dag"
    | "cancelled";
  content?: string;
  message?: string;
  usage?: { input_tokens: number; output_tokens: number };
  cost?: number;
  id?: string;
  tool_name?: string;
  arguments_summary?: string;
  elevation?: boolean;
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
  dag_id?: string;
  fallback?: boolean;
  outline?: string;
  nodes?: DagNodeFrame[];
}

export interface DagNodeFrame {
  id: string;
  label: string;
  task_type: string;
  caps: string;
  contact?: string;
  status: string;
}

export interface DagFrame {
  dag_id: string;
  fallback: boolean;
  outline: string;
  nodes: DagNodeFrame[];
}

export interface ApprovalRequiredPayload {
  id: string;
  tool_name: string;
  arguments_summary: string;
  elevation?: boolean;
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
  /** CLI `--plan` equivalent. Default `"build"`. */
  hostPhase?: "plan" | "build";
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
  onDag?: (payload: DagFrame) => void;
  onCancelled?: (message?: string) => void;
}

function wsUrl(token: string): string {
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  const host = window.location.host;
  const q = token ? `?token=${encodeURIComponent(token)}` : "";
  return `${proto}//${host}/ws${q}`;
}

export function chatClientFrame(opts: {
  messages: ChatMessage[];
  sessionId?: string;
  modelId?: string;
  temperature?: number;
  hostPhase?: "plan" | "build";
}): Record<string, unknown> {
  return {
    type: "chat",
    session_id: opts.sessionId,
    messages: opts.messages,
    model_id: opts.modelId,
    temperature: opts.temperature,
    host_phase: opts.hostPhase ?? "build",
  };
}

export function streamChat(opts: StreamChatOptions): () => void {
  const socket = new WebSocket(wsUrl(opts.token));
  let closed = false;

  socket.addEventListener("open", () => {
    socket.send(
      JSON.stringify(
        chatClientFrame({
          messages: opts.messages,
          sessionId: opts.sessionId,
          modelId: opts.modelId,
          temperature: opts.temperature,
          hostPhase: opts.hostPhase,
        }),
      ),
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
    } else if (frame.type === "dag" && frame.dag_id && Array.isArray(frame.nodes)) {
      opts.onDag?.({
        dag_id: frame.dag_id,
        fallback: frame.fallback === true,
        outline: frame.outline ?? "",
        nodes: frame.nodes,
      });
    } else if (frame.type === "cancelled") {
      opts.onCancelled?.(frame.message);
      socket.close();
    } else if (frame.type === "approval_required" && frame.id && frame.tool_name) {
      opts.onApprovalRequired?.({
        id: frame.id,
        tool_name: frame.tool_name,
        arguments_summary: frame.arguments_summary ?? "",
        elevation: frame.elevation === true,
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
  // In-flight model/run status is ephemeral; drop it once the reply starts.
  while (out.length > 0 && out[out.length - 1]?.role === "status") {
    out.pop();
  }
  const last = out[out.length - 1];
  if (last?.role === "assistant") {
    out[out.length - 1] = { role: "assistant", content: last.content + delta };
  } else {
    out.push({ role: "assistant", content: delta });
  }
  return out;
}

/** Drop ephemeral status lines (e.g. after done / cancel). */
export function clearStatusMessages(messages: ChatMessage[]): ChatMessage[] {
  return messages.filter((m) => m.role !== "status");
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

export type LiveDagPreviewNode = {
  index: number;
  id: string;
  taskType: string;
  caps: string;
  next: string;
};

export type LiveDagPreview = {
  dagId: string;
  fallback: boolean;
  nodes: LiveDagPreviewNode[];
};

/** Parse Plan-phase bounded DAG preview (VL-NA-015/016 product chrome). */
export function parseLiveDagPreview(text: string): LiveDagPreview | null {
  const dagId = text.match(/Bounded task DAG `([^`]+)`/)?.[1];
  if (!dagId) {
    return null;
  }
  const nodes: LiveDagPreviewNode[] = [];
  const re =
    /^(\d+)\.\s+(\S+)\s+task_type=(\S+)\s+caps=(\S*)\s+next=(\S+)\s*$/gm;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    nodes.push({
      index: Number(m[1]),
      id: m[2],
      taskType: m[3],
      caps: m[4],
      next: m[5],
    });
  }
  if (nodes.length === 0) {
    return null;
  }
  return {
    dagId,
    fallback: text.includes("using handwritten fallback"),
    nodes,
  };
}
