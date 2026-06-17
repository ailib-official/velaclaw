export type ChatRole = "user" | "assistant";

export interface ChatMessage {
  role: ChatRole;
  content: string;
}

export interface WsServerFrame {
  type: "delta" | "done" | "error";
  content?: string;
  message?: string;
  usage?: { input_tokens: number; output_tokens: number };
  cost?: number;
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
