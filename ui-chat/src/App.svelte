<script lang="ts">
  import { onMount } from "svelte";
  import {
    appendAssistantDelta,
    streamChat,
    type ChatMessage,
  } from "./lib/chat";
  import {
    createCronJob,
    createSession,
    deleteCronJob,
    deleteSession,
    fetchConfig,
    fetchCronJobs,
    fetchHealth,
    fetchMemory,
    fetchProviders,
    fetchSession,
    fetchSessions,
    fetchTools,
    loadToken,
    putConfig,
    respondApproval,
    runCronJob,
    saveToken,
    testProvider,
    type CronJob,
    type MemoryEntry,
    type ProviderModel,
    type SessionSummary,
    type ToolCatalogEntry,
  } from "./lib/api";
  import type { ApprovalRequiredPayload } from "./lib/chat";

  type Tab = "chat" | "memory" | "cron" | "tools" | "settings";

  let token = $state(loadToken());
  let tab = $state<Tab>("chat");
  let models = $state<ProviderModel[]>([]);
  let selectedModel = $state("");
  let messages = $state<ChatMessage[]>([]);
  let sessions = $state<SessionSummary[]>([]);
  let activeSessionId = $state<string | null>(null);
  let input = $state("");
  let streaming = $state(false);
  let status = $state("connecting");
  let toast = $state("");
  let cancelStream: (() => void) | null = null;

  let memoryQuery = $state("");
  let memoryEntries = $state<MemoryEntry[]>([]);
  let configModel = $state("");
  let configTemperature = $state("0.7");
  let aiProtocolDir = $state("");

  let cronJobs = $state<CronJob[]>([]);
  let cronExpression = $state("0 9 * * *");
  let cronCommand = $state("");
  let toolCatalog = $state<ToolCatalogEntry[]>([]);
  let pendingApproval = $state<ApprovalRequiredPayload | null>(null);
  let providerTestMsg = $state("");
  let providers = $state<{ id: string; available: boolean }[]>([]);

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
      const providersRes = await fetchProviders(token);
      models = providersRes.models.filter((m) => m.available);
      providers = providersRes.providers;
      if (!selectedModel && models.length > 0) {
        selectedModel = models[0].logical_id;
      }
    } catch (e) {
      status = "offline";
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function loadSessions() {
    if (!token) return;
    try {
      sessions = await fetchSessions(token);
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function selectSession(id: string) {
    try {
      const detail = await fetchSession(token, id);
      activeSessionId = detail.id;
      messages = detail.messages.map((m) => ({
        role: m.role as ChatMessage["role"],
        content: m.content,
      }));
      if (detail.model_id) selectedModel = detail.model_id;
      scrollToBottom();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function newSession() {
    try {
      const session = await createSession(token, undefined, selectedModel || undefined);
      activeSessionId = session.id;
      messages = [];
      await loadSessions();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function removeSession(id: string) {
    try {
      await deleteSession(token, id);
      if (activeSessionId === id) {
        activeSessionId = null;
        messages = [];
      }
      await loadSessions();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function loadMemory() {
    if (!token) return;
    try {
      const data = await fetchMemory(token, memoryQuery || undefined);
      memoryEntries = data.entries;
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function loadCron() {
    if (!token) return;
    try {
      cronJobs = await fetchCronJobs(token);
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function addCronJob() {
    if (!cronExpression.trim() || !cronCommand.trim()) return;
    try {
      await createCronJob(token, {
        expression: cronExpression.trim(),
        command: cronCommand.trim(),
      });
      cronCommand = "";
      await loadCron();
      showToast("Cron job created");
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function removeCronJob(id: string) {
    try {
      await deleteCronJob(token, id);
      await loadCron();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function triggerCronJob(id: string) {
    try {
      const result = await runCronJob(token, id);
      showToast(result.success ? "Job ran OK" : `Run failed: ${result.output}`);
      await loadCron();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function loadTools() {
    if (!token) return;
    try {
      toolCatalog = await fetchTools(token);
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleApproval(approved: boolean, always = false) {
    if (!pendingApproval) return;
    const id = pendingApproval.id;
    try {
      await respondApproval(token, id, approved, always);
      pendingApproval = null;
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function runProviderTest(providerId: string) {
    providerTestMsg = "Testing…";
    try {
      const result = await testProvider(token, providerId);
      providerTestMsg = result.ok
        ? `${providerId}: OK`
        : `${providerId}: ${result.message ?? "failed"}`;
    } catch (e) {
      providerTestMsg = e instanceof Error ? e.message : String(e);
    }
  }

  async function loadSettings() {
    if (!token) return;
    try {
      const cfg = await fetchConfig(token);
      configModel = String(cfg.default_model ?? "");
      configTemperature = String(cfg.default_temperature ?? "0.7");
      aiProtocolDir = String(
        (cfg as { runtime?: { ai_protocol_dir?: string } }).runtime?.ai_protocol_dir ?? "",
      );
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function saveSettings() {
    try {
      const temp = parseFloat(configTemperature);
      await putConfig(token, {
        default_model: configModel || null,
        default_temperature: Number.isFinite(temp) ? temp : 0.7,
      });
      showToast("Settings saved");
      await refreshMeta();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  onMount(() => {
    refreshMeta();
    loadSessions();
  });

  function saveTokenAndRefresh() {
    saveToken(token);
    refreshMeta();
    loadSessions();
  }

  function switchTab(next: Tab) {
    tab = next;
    if (next === "memory") loadMemory();
    if (next === "cron") loadCron();
    if (next === "tools") loadTools();
    if (next === "settings") loadSettings();
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

  async function ensureSession() {
    if (activeSessionId) return activeSessionId;
    const session = await createSession(token, undefined, selectedModel || undefined);
    activeSessionId = session.id;
    await loadSessions();
    return session.id;
  }

  async function send() {
    const text = input.trim();
    if (!text || streaming) return;
    input = "";
    messages = [...messages, { role: "user", content: text }];
    streaming = true;
    scrollToBottom();

    let sessionId: string | undefined;
    try {
      sessionId = await ensureSession();
    } catch (e) {
      streaming = false;
      showToast(e instanceof Error ? e.message : String(e));
      return;
    }

    const history = messages;
    cancelStream = streamChat({
      token,
      sessionId,
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
        loadSessions();
      },
      onError: (msg) => {
        streaming = false;
        cancelStream = null;
        showToast(msg);
      },
      onApprovalRequired: (payload) => {
        pendingApproval = payload;
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
    <h1>VelaClaw</h1>
    <nav class="tabs">
      <button type="button" class:active={tab === "chat"} onclick={() => switchTab("chat")}>Chat</button>
      <button type="button" class:active={tab === "memory"} onclick={() => switchTab("memory")}>Memory</button>
      <button type="button" class:active={tab === "cron"} onclick={() => switchTab("cron")}>Cron</button>
      <button type="button" class:active={tab === "tools"} onclick={() => switchTab("tools")}>Tools</button>
      <button type="button" class:active={tab === "settings"} onclick={() => switchTab("settings")}>Settings</button>
      <a class="dash-link" href="/dashboard" target="_blank" rel="noopener">Dashboard ↗</a>
    </nav>
    <span class="badge" class:ok={status === "online"}>{status}</span>
  </header>

  <section class="toolbar">
    <label>
      Bearer token
      <input type="password" bind:value={token} placeholder="from POST /pair" />
    </label>
    <button type="button" onclick={saveTokenAndRefresh}>Save token</button>
    {#if tab === "chat"}
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
    {/if}
  </section>

  {#if tab === "chat"}
    <div class="chat-grid">
      <aside class="sessions">
        <div class="sessions-head">
          <strong>Sessions</strong>
          <button type="button" onclick={newSession}>+ New</button>
        </div>
        <ul>
          {#each sessions as s}
            <li class:active={s.id === activeSessionId}>
              <button type="button" class="session-title" onclick={() => selectSession(s.id)}>
                {s.title}
              </button>
              <button type="button" class="session-del" onclick={() => removeSession(s.id)} title="Delete">×</button>
            </li>
          {/each}
        </ul>
      </aside>

      <div class="chat-main">
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
      </div>
    </div>
  {:else if tab === "memory"}
    <section class="panel">
      <div class="panel-head">
        <input type="search" bind:value={memoryQuery} placeholder="Search memory…" />
        <button type="button" onclick={loadMemory}>Search</button>
      </div>
      <ul class="memory-list">
        {#each memoryEntries as entry}
          <li>
            <div class="mem-meta">{entry.category} · {entry.timestamp}</div>
            <div class="mem-key">{entry.key}</div>
            <div class="mem-body">{entry.content}</div>
          </li>
        {/each}
      </ul>
    </section>
  {:else if tab === "cron"}
    <section class="panel">
      <div class="panel-head cron-form">
        <label>
          Cron expression
          <input type="text" bind:value={cronExpression} placeholder="0 9 * * *" />
        </label>
        <label>
          Command
          <input type="text" bind:value={cronCommand} placeholder="velaclaw agent …" />
        </label>
        <button type="button" onclick={addCronJob}>Add job</button>
      </div>
      <ul class="cron-list">
        {#each cronJobs as job}
          <li>
            <div class="cron-meta">
              <code>{job.expression}</code>
              {#if job.last_status}
                <span class="cron-status">{job.last_status}</span>
              {/if}
            </div>
            <div class="cron-cmd">{job.command}</div>
            <div class="cron-actions">
              <button type="button" onclick={() => triggerCronJob(job.id)}>Run now</button>
              <button type="button" class="danger" onclick={() => removeCronJob(job.id)}>Delete</button>
            </div>
          </li>
        {/each}
        {#if cronJobs.length === 0}
          <li class="empty">No cron jobs yet.</li>
        {/if}
      </ul>
    </section>
  {:else if tab === "tools"}
    <section class="panel">
      <p class="hint">Tools available to the agent at runtime (from gateway config).</p>
      <ul class="tools-list">
        {#each toolCatalog as tool}
          <li>
            <div class="tool-name">{tool.name}</div>
            <div class="tool-desc">{tool.description}</div>
          </li>
        {/each}
        {#if toolCatalog.length === 0}
          <li class="empty">No tools loaded — check token and gateway.</li>
        {/if}
      </ul>
    </section>
  {:else}
    <section class="panel settings">
      <p class="hint">API keys cannot be set here — configure BYOK via environment variables.</p>
      <label>
        Default model
        <input type="text" bind:value={configModel} placeholder="provider/model" />
      </label>
      <label>
        Temperature
        <input type="text" bind:value={configTemperature} />
      </label>
      <label>
        AI_PROTOCOL_DIR (read-only)
        <input type="text" value={aiProtocolDir} readonly />
      </label>
      <button type="button" onclick={saveSettings}>Save settings</button>
      {#if providers.length > 0}
        <div class="provider-tests">
          <strong>Provider connectivity</strong>
          {#each providers as p}
            <div class="provider-row">
              <span>{p.id}</span>
              <span class:ok={p.available} class:bad={!p.available}>{p.available ? "BYOK OK" : "missing env"}</span>
              <button type="button" onclick={() => runProviderTest(p.id)} disabled={!p.available}>Test</button>
            </div>
          {/each}
          {#if providerTestMsg}
            <p class="hint">{providerTestMsg}</p>
          {/if}
        </div>
      {/if}
    </section>
  {/if}

  {#if pendingApproval}
    <div class="modal-backdrop" role="presentation">
      <div class="modal" role="dialog" aria-labelledby="approval-title">
        <h2 id="approval-title">Tool approval required</h2>
        <p><strong>{pendingApproval.tool_name}</strong></p>
        <pre class="approval-args">{pendingApproval.arguments_summary}</pre>
        <div class="modal-actions">
          <button type="button" class="danger" onclick={() => handleApproval(false)}>Deny</button>
          <button type="button" onclick={() => handleApproval(true)}>Allow once</button>
          <button type="button" onclick={() => handleApproval(true, true)}>Always allow</button>
        </div>
      </div>
    </div>
  {/if}

  {#if toast}
    <div class="toast" role="alert">{toast}</div>
  {/if}
</div>

<style>
  .layout {
    max-width: 1100px;
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
    flex-wrap: wrap;
  }
  h1 {
    margin: 0;
    font-size: 1.25rem;
    color: #38bdf8;
  }
  .tabs {
    display: flex;
    gap: 0.25rem;
  }
  .tabs button {
    background: #1e293b;
    color: #94a3b8;
    padding: 0.35rem 0.75rem;
  }
  .tabs button.active {
    background: #0284c7;
    color: white;
  }
  .dash-link {
    font-size: 0.8rem;
    color: #38bdf8;
    text-decoration: none;
    padding: 0.35rem 0.5rem;
  }
  .badge {
    font-size: 0.75rem;
    padding: 0.15rem 0.5rem;
    border-radius: 999px;
    background: #334155;
    margin-left: auto;
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
  .chat-grid {
    display: grid;
    grid-template-columns: 220px 1fr;
    gap: 0.75rem;
    flex: 1;
    min-height: 400px;
  }
  .sessions {
    background: #1e293b;
    border-radius: 8px;
    padding: 0.5rem;
    overflow-y: auto;
  }
  .sessions-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.5rem;
    font-size: 0.85rem;
  }
  .sessions ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .sessions li {
    display: flex;
    gap: 0.25rem;
    margin-bottom: 0.25rem;
  }
  .sessions li.active .session-title {
    background: #0c4a6e;
  }
  .session-title {
    flex: 1;
    text-align: left;
    font-size: 0.8rem;
    background: #334155;
    padding: 0.4rem 0.5rem;
  }
  .session-del {
    padding: 0.25rem 0.5rem;
    background: #475569;
  }
  .chat-main {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-height: 0;
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
  .panel {
    background: #1e293b;
    border-radius: 8px;
    padding: 1rem;
    flex: 1;
  }
  .panel-head {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }
  .panel-head input {
    flex: 1;
  }
  .memory-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .memory-list li {
    background: #334155;
    border-radius: 8px;
    padding: 0.75rem;
  }
  .mem-meta {
    font-size: 0.7rem;
    color: #94a3b8;
  }
  .mem-key {
    font-weight: 600;
    margin: 0.25rem 0;
  }
  .mem-body {
    font-size: 0.9rem;
    white-space: pre-wrap;
  }
  .settings {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    max-width: 480px;
  }
  .hint {
    font-size: 0.85rem;
    color: #94a3b8;
    margin: 0;
  }
  .cron-form {
    flex-wrap: wrap;
    align-items: end;
  }
  .cron-list,
  .tools-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .cron-list li,
  .tools-list li {
    background: #334155;
    border-radius: 8px;
    padding: 0.75rem;
  }
  .cron-list li.empty,
  .tools-list li.empty {
    color: #94a3b8;
    font-size: 0.9rem;
  }
  .cron-meta {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    font-size: 0.85rem;
  }
  .cron-status {
    color: #86efac;
    font-size: 0.75rem;
  }
  .cron-cmd {
    margin: 0.35rem 0;
    font-family: monospace;
    font-size: 0.85rem;
  }
  .cron-actions {
    display: flex;
    gap: 0.5rem;
  }
  button.danger {
    background: #b91c1c;
  }
  .tool-name {
    font-weight: 600;
    font-family: monospace;
  }
  .tool-desc {
    font-size: 0.85rem;
    color: #cbd5e1;
    margin-top: 0.25rem;
  }
  .provider-tests {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }
  .provider-row {
    display: flex;
    gap: 0.75rem;
    align-items: center;
    font-size: 0.85rem;
  }
  .provider-row .ok {
    color: #86efac;
  }
  .provider-row .bad {
    color: #fca5a5;
  }
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(15, 23, 42, 0.75);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .modal {
    background: #1e293b;
    border: 1px solid #475569;
    border-radius: 12px;
    padding: 1.25rem;
    max-width: 480px;
    width: 90%;
  }
  .modal h2 {
    margin: 0 0 0.75rem;
    font-size: 1.1rem;
  }
  .approval-args {
    background: #0f172a;
    padding: 0.75rem;
    border-radius: 6px;
    overflow-x: auto;
    font-size: 0.8rem;
    white-space: pre-wrap;
  }
  .modal-actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 1rem;
    flex-wrap: wrap;
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
  @media (max-width: 720px) {
    .chat-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
