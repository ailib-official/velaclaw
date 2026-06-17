<script lang="ts">
  import { onMount } from "svelte";
  import {
    appendAssistantDelta,
    streamChat,
    type ChatMessage,
  } from "./lib/chat";
  import {
    fetchHealth,
    fetchProviders,
    loadToken,
    saveToken,
    type ProviderModel,
  } from "./lib/api";

  let token = $state(loadToken());
  let models = $state<ProviderModel[]>([]);
  let selectedModel = $state("");
  let messages = $state<ChatMessage[]>([]);
  let input = $state("");
  let streaming = $state(false);
  let status = $state("connecting");
  let toast = $state("");
  let cancelStream: (() => void) | null = null;

  let listEl: HTMLDivElement | undefined;

  function showToast(msg: string) {
    toast = msg;
    setTimeout(() => {
      if (toast === msg) toast = "";
    }, 5000);
  }

  function scrollToBottom() {
    queueMicrotask(() => listEl?.scrollTo({ top: listEl.scrollHeight, behavior: "smooth" }));
  }

  async function refreshMeta() {
    try {
      const health = await fetchHealth();
      status = health.status === "ok" ? "online" : "degraded";
      const providers = await fetchProviders(token);
      models = providers.models.filter((m) => m.available);
      if (!selectedModel && models.length > 0) {
        selectedModel = models[0].logical_id;
      }
    } catch (e) {
      status = "offline";
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  onMount(() => {
    refreshMeta();
  });

  function saveTokenAndRefresh() {
    saveToken(token);
    refreshMeta();
  }

  function renderMarkdown(text: string): string {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\n/g, "<br/>");
  }

  function send() {
    const text = input.trim();
    if (!text || streaming) return;
    input = "";
    messages = [...messages, { role: "user", content: text }];
    streaming = true;
    scrollToBottom();

    const history = messages;
    cancelStream = streamChat({
      token,
      messages: history,
      modelId: selectedModel || undefined,
      onDelta: (chunk) => {
        messages = appendAssistantDelta(messages, chunk);
        scrollToBottom();
      },
      onDone: () => {
        streaming = false;
        cancelStream = null;
        scrollToBottom();
      },
      onError: (msg) => {
        streaming = false;
        cancelStream = null;
        showToast(msg);
      },
    });
  }

  function onKeydown(ev: KeyboardEvent) {
    if (ev.key === "Enter" && !ev.shiftKey) {
      ev.preventDefault();
      send();
    }
  }
</script>

<div class="layout">
  <header>
    <h1>VelaClaw Chat</h1>
    <span class="badge" class:ok={status === "online"}>{status}</span>
  </header>

  <section class="toolbar">
    <label>
      Bearer token
      <input type="password" bind:value={token} placeholder="from POST /pair" />
    </label>
    <button type="button" onclick={saveTokenAndRefresh}>Save token</button>
    <label>
      Model
      <select bind:value={selectedModel} disabled={models.length === 0}>
        {#if models.length === 0}
          <option value="">No models (check BYOK)</option>
        {:else}
          {#each models as m}
            <option value={m.logical_id}>{m.logical_id}</option>
          {/each}
        {/if}
      </select>
    </label>
  </section>

  <div class="messages" bind:this={listEl}>
    {#each messages as msg}
      <article class={msg.role}>
        <div class="role">{msg.role}</div>
        {#if msg.role === "assistant"}
          <div class="body">{@html renderMarkdown(msg.content)}</div>
        {:else}
          <div class="body">{msg.content}</div>
        {/if}
      </article>
    {/each}
    {#if streaming}
      <p class="typing">Streaming…</p>
    {/if}
  </div>

  <footer>
    <textarea
      rows="3"
      bind:value={input}
      onkeydown={onKeydown}
      placeholder="Message… (Enter to send)"
      disabled={streaming}
    ></textarea>
    <button type="button" onclick={send} disabled={streaming || !input.trim()}>Send</button>
  </footer>

  {#if toast}
    <div class="toast" role="alert">{toast}</div>
  {/if}
</div>

<style>
  .layout {
    max-width: 900px;
    margin: 0 auto;
    padding: 1rem;
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }
  h1 {
    margin: 0;
    font-size: 1.25rem;
    color: #38bdf8;
  }
  .badge {
    font-size: 0.75rem;
    padding: 0.15rem 0.5rem;
    border-radius: 999px;
    background: #334155;
  }
  .badge.ok {
    background: #14532d;
    color: #86efac;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: end;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.75rem;
    color: #94a3b8;
  }
  input,
  select,
  textarea {
    background: #1e293b;
    border: 1px solid #334155;
    border-radius: 6px;
    color: inherit;
    padding: 0.5rem;
  }
  button {
    background: #0284c7;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.5rem 1rem;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .messages {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    min-height: 300px;
    padding: 0.5rem;
    background: #1e293b;
    border-radius: 8px;
  }
  article {
    padding: 0.5rem 0.75rem;
    border-radius: 8px;
    max-width: 85%;
  }
  article.user {
    align-self: flex-end;
    background: #0c4a6e;
  }
  article.assistant {
    align-self: flex-start;
    background: #334155;
  }
  .role {
    font-size: 0.65rem;
    text-transform: uppercase;
    color: #94a3b8;
    margin-bottom: 0.25rem;
  }
  .body :global(code) {
    background: #0f172a;
    padding: 0.1rem 0.3rem;
    border-radius: 4px;
  }
  footer {
    display: flex;
    gap: 0.5rem;
    align-items: stretch;
  }
  textarea {
    flex: 1;
    resize: vertical;
  }
  .typing {
    color: #94a3b8;
    font-size: 0.85rem;
    margin: 0;
  }
  .toast {
    position: fixed;
    bottom: 1rem;
    right: 1rem;
    background: #7f1d1d;
    color: #fecaca;
    padding: 0.75rem 1rem;
    border-radius: 8px;
    max-width: 360px;
  }
</style>
