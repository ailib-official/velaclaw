<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import {
    appendAssistantDelta,
    appendSystemNotice,
    applyStatusFrame,
    applyStepFrame,
    clearStatusMessages,
    lastAssistantHasVelaClawNotice,
    listenSessionEvents,
    looksLikeVelaClawNotice,
    outboundChatHistory,
    streamChat,
    type ChatMessage,
    type DagFrame,
  } from "./lib/chat";
  import {
    createCronJob,
    createSession,
    deleteCronJob,
    deleteSession,
    fetchConfig,
    fetchCronJobs,
    fetchDashboard,
    fetchHealth,
    fetchMemory,
    fetchProviders,
    fetchSession,
    fetchSessions,
    fetchTools,
    loadToken,
    putConfig,
    respondApproval,
    respondHumanInput,
    runCronJob,
    saveToken,
    testProvider,
    type CronJob,
    type MemoryEntry,
    type ProviderModel,
    type SessionSummary,
    type ToolCatalogEntry,
  } from "./lib/api";
  import {
    dashboardViewFromPayload,
    formatInt,
    formatUsd,
    type DashboardView,
  } from "./lib/dashboard";
  import { renderMarkdown } from "./lib/markdown";
  import type { ApprovalRequiredPayload, HumanInputRequiredPayload } from "./lib/chat";
  import {
    applySessionTitle,
    formatSessionMeta,
    resolveInitialSessionId,
    saveActiveSessionId,
    syncSessionUrl,
  } from "./lib/sessions";
  import {
    formatRoutingSummary,
    routingDiagnosticsFromConfig,
    type RoutingDiagnosticsView,
  } from "./lib/diagnostics";

  type Tab = "overview" | "chat" | "sessions" | "memory" | "cron" | "tools" | "settings";

  let token = $state(loadToken());
  let tab = $state<Tab>("chat");
  let models = $state<ProviderModel[]>([]);
  let selectedModel = $state("");
  let modelPickerAttention = $state(false);
  let messages = $state<ChatMessage[]>([]);
  let sessions = $state<SessionSummary[]>([]);
  let activeSessionId = $state<string | null>(null);
  let input = $state("");
  let streaming = $state(false);
  let status = $state("connecting");
  let toast = $state("");
  let cancelStream: (() => void) | null = null;
  let stopSessionEvents: (() => void) | null = null;

  let memoryQuery = $state("");
  let memoryEntries = $state<MemoryEntry[]>([]);
  let configModel = $state("");
  let configTemperature = $state("0.7");
  let aiProtocolDir = $state("");

  let cronJobs = $state<CronJob[]>([]);
  let cronExpression = $state("0 9 * * *");
  let cronCommand = $state("");
  let toolCatalog = $state<ToolCatalogEntry[]>([]);
  let pendingApprovals = $state<ApprovalRequiredPayload[]>([]);
  let pendingHumanInput = $state<HumanInputRequiredPayload | null>(null);
  let humanInputText = $state("");
  let humanInputSecret = $state("");
  let providerTestMsg = $state("");
  let providers = $state<{ id: string; available: boolean }[]>([]);
  let dashboardView = $state<DashboardView | null>(null);
  let dashboardLoading = $state(false);
  let routingDiagnostics = $state<RoutingDiagnosticsView | null>(null);
  let hostPhase = $state<"plan" | "build">("build");
  let boundedDagLive = $state(false);
  let liveDag = $state<DagFrame | null>(null);
  let liveDagOutlinePosted = $state(false);

  let listEl: HTMLDivElement | undefined;
  let inputEl: HTMLTextAreaElement | undefined;
  let modelSelectEl: HTMLSelectElement | undefined;

  /** Return focus to the chat composer after a turn ends (textarea was disabled while streaming). */
  async function focusChatInput() {
    if (tab !== "chat" || pendingApprovals.length > 0 || pendingHumanInput) return;
    await tick();
    inputEl?.focus();
  }

  async function highlightModelPicker() {
    modelPickerAttention = true;
    await tick();
    modelSelectEl?.focus();
    setTimeout(() => {
      modelPickerAttention = false;
    }, 12000);
  }

  function showToast(msg: string, ms = 5000) {
    toast = msg;
    setTimeout(() => {
      if (toast === msg) toast = "";
    }, ms);
  }

  function surfaceSoftFailUx(msg: string, opts: { persistSystem?: boolean } = {}) {
    const { persistSystem = true } = opts;
    if (persistSystem) {
      messages = appendSystemNotice(messages, msg);
      scrollToBottom();
    }
    showToast(msg, 10000);
    void highlightModelPicker();
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

  function applyPushedSessionTitle(sessionId: string, title: string) {
    sessions = applySessionTitle(sessions, sessionId, title);
  }

  function startSessionEvents() {
    stopSessionEvents?.();
    stopSessionEvents = null;
    if (!token) return;
    stopSessionEvents = listenSessionEvents({
      token,
      onSessionTitle: applyPushedSessionTitle,
    });
  }

  function stopStreaming() {
    if (cancelStream) {
      cancelStream();
      cancelStream = null;
    }
    streaming = false;
    pendingApprovals = [];
    pendingHumanInput = null;
    void focusChatInput();
  }

  function trackActiveSession(id: string | null) {
    saveActiveSessionId(id);
    syncSessionUrl(id);
  }

  async function loadSessions() {
    if (!token) {
      sessions = [];
      return;
    }
    try {
      sessions = await fetchSessions(token);
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function selectSession(id: string, options: { switchToChat?: boolean } = {}) {
    const { switchToChat = true } = options;
    if (streaming && activeSessionId !== id) {
      stopStreaming();
    }
    try {
      const detail = await fetchSession(token, id);
      activeSessionId = detail.id;
      trackActiveSession(detail.id);
      messages = detail.messages.map((m) => ({
        role: m.role as ChatMessage["role"],
        content: m.content,
      }));
      if (detail.model_id) selectedModel = detail.model_id;
      scrollToBottom();
      if (switchToChat && tab !== "chat") {
        switchTab("chat");
      }
    } catch (e) {
      if (resolveInitialSessionId(window.location.search) === id) {
        trackActiveSession(null);
      }
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function resumeInitialSession() {
    if (!token) return;
    const id = resolveInitialSessionId(window.location.search);
    if (!id) return;
    await selectSession(id, { switchToChat: false });
  }

  async function newSession() {
    try {
      const session = await createSession(token, undefined, selectedModel || undefined);
      activeSessionId = session.id;
      trackActiveSession(session.id);
      messages = [];
      await loadSessions();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function newSessionAndOpen() {
    await newSession();
    switchTab("chat");
  }

  function openSessionInChat(id: string) {
    void selectSession(id, { switchToChat: true });
  }

  async function removeSession(id: string) {
    try {
      await deleteSession(token, id);
      if (activeSessionId === id) {
        activeSessionId = null;
        messages = [];
        trackActiveSession(null);
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

  async function handleApproval(approved: boolean, always = false, never = false) {
    const current = pendingApprovals[0];
    if (!current) return;
    const id = current.id;
    try {
      await respondApproval(token, id, approved, always, never);
      pendingApprovals = pendingApprovals.slice(1);
      if (!streaming && pendingApprovals.length === 0) await focusChatInput();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function clearHumanInputModal() {
    pendingHumanInput = null;
    humanInputText = "";
    humanInputSecret = "";
    if (!streaming) await focusChatInput();
  }

  async function handleHumanInputCancel() {
    if (!pendingHumanInput) return;
    const id = pendingHumanInput.id;
    try {
      await respondHumanInput(token, id, { cancelled: true });
      await clearHumanInputModal();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleHumanInputChoice(selected: string) {
    if (!pendingHumanInput) return;
    const id = pendingHumanInput.id;
    try {
      await respondHumanInput(token, id, { selected });
      await clearHumanInputModal();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleHumanInputTextSubmit() {
    if (!pendingHumanInput) return;
    const text = humanInputText.trim();
    if (!text) {
      showToast("Enter a value");
      return;
    }
    if ([...text].length > 128) {
      showToast("Short codes only (max 128 characters)");
      return;
    }
    const id = pendingHumanInput.id;
    try {
      await respondHumanInput(token, id, { text });
      await clearHumanInputModal();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleHumanInputSecretSubmit() {
    if (!pendingHumanInput) return;
    const secret = humanInputSecret;
    if (!secret) {
      showToast("Enter a secret");
      return;
    }
    const id = pendingHumanInput.id;
    try {
      await respondHumanInput(token, id, { secret });
      await clearHumanInputModal();
    } catch (e) {
      showToast(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleHumanInputHandoffDone() {
    if (!pendingHumanInput) return;
    const id = pendingHumanInput.id;
    try {
      await respondHumanInput(token, id, {});
      await clearHumanInputModal();
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

  async function loadChatAgentFlags() {
    if (!token) {
      boundedDagLive = false;
      return;
    }
    try {
      const cfg = await fetchConfig(token);
      const agent = cfg.agent as Record<string, unknown> | undefined;
      boundedDagLive = agent?.bounded_dag_live === true;
      if (boundedDagLive) {
        hostPhase = "build";
      }
    } catch {
      boundedDagLive = false;
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
      routingDiagnostics = routingDiagnosticsFromConfig(cfg as Record<string, unknown>);
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

  async function loadOverview() {
    dashboardLoading = true;
    try {
      const payload = await fetchDashboard(token);
      dashboardView = dashboardViewFromPayload(payload);
    } catch (e) {
      dashboardView = null;
      showToast(e instanceof Error ? e.message : String(e));
    } finally {
      dashboardLoading = false;
    }
  }

  onMount(() => {
    startSessionEvents();
    void (async () => {
      await refreshMeta();
      await loadSessions();
      await loadChatAgentFlags();
      const params = new URLSearchParams(window.location.search);
      const requested = params.get("tab");
      const tabs: Tab[] = ["overview", "chat", "sessions", "memory", "cron", "tools", "settings"];
      if (requested && tabs.includes(requested as Tab)) {
        switchTab(requested as Tab);
      } else {
        await resumeInitialSession();
      }
    })();
  });

  onDestroy(() => {
    stopSessionEvents?.();
    stopSessionEvents = null;
  });

  async function saveTokenAndRefresh() {
    saveToken(token);
    startSessionEvents();
    await refreshMeta();
    await loadSessions();
    await resumeInitialSession();
  }

  function switchTab(next: Tab) {
    tab = next;
    const url = new URL(window.location.href);
    if (next === "chat") {
      url.searchParams.delete("tab");
    } else {
      url.searchParams.set("tab", next);
    }
    window.history.replaceState({}, "", `${url.pathname}${url.search}`);
    if (next === "overview") loadOverview();
    if (next === "chat" || next === "sessions") {
      void loadSessions();
      if (next === "chat") void loadChatAgentFlags();
    }
    if (next === "memory") loadMemory();
    if (next === "cron") loadCron();
    if (next === "tools") loadTools();
    if (next === "settings") loadSettings();
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
    pendingApprovals = [];
    pendingHumanInput = null;
    humanInputText = "";
    humanInputSecret = "";
    streaming = true;
    liveDag = null;
    liveDagOutlinePosted = false;
    scrollToBottom();

    let sessionId: string | undefined;
    try {
      sessionId = await ensureSession();
    } catch (e) {
      streaming = false;
      showToast(e instanceof Error ? e.message : String(e));
      await focusChatInput();
      return;
    }

    const history = outboundChatHistory(messages);
    cancelStream = streamChat({
      token,
      sessionId,
      messages: history,
      modelId: selectedModel || undefined,
      hostPhase: boundedDagLive ? "build" : hostPhase,
      onDelta: (chunk) => {
        messages = appendAssistantDelta(messages, chunk);
        scrollToBottom();
      },
      onStatus: (phase, detail) => {
        messages = applyStatusFrame(messages, phase, detail);
        scrollToBottom();
      },
      onStep: (payload) => {
        messages = applyStepFrame(messages, payload);
        scrollToBottom();
      },
      onDag: (payload) => {
        liveDag = payload;
        if (payload.outline && !liveDagOutlinePosted) {
          liveDagOutlinePosted = true;
          messages = appendAssistantDelta(messages, payload.outline);
        }
        scrollToBottom();
      },
      onCancelled: (msg) => {
        streaming = false;
        cancelStream = null;
        messages = clearStatusMessages(appendSystemNotice(messages, msg || "Stopped."));
        scrollToBottom();
        pendingApprovals = [];
        pendingHumanInput = null;
        void focusChatInput();
      },
      onSessionTitle: applyPushedSessionTitle,
      onDone: () => {
        streaming = false;
        cancelStream = null;
        messages = clearStatusMessages(messages);
        scrollToBottom();
        // Immediate refresh + deferred picks up async session-title refine.
        void loadSessions();
        window.setTimeout(() => void loadSessions(), 2500);
        if (lastAssistantHasVelaClawNotice(messages)) {
          showToast(
            "Model soft-fail notice in reply — consider switching model in the picker.",
            10000,
          );
          void highlightModelPicker();
        } else {
          void focusChatInput();
        }
      },
      onError: (msg) => {
        streaming = false;
        cancelStream = null;
        messages = clearStatusMessages(messages);
        if (looksLikeVelaClawNotice(msg)) {
          surfaceSoftFailUx(msg);
        } else {
          messages = appendSystemNotice(messages, msg);
          scrollToBottom();
          showToast(msg);
          void focusChatInput();
        }
      },
      onApprovalRequired: (payload) => {
        pendingApprovals = [...pendingApprovals, payload];
      },
      onInputRequired: (payload) => {
        pendingHumanInput = payload;
        humanInputText = "";
        humanInputSecret = "";
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
      <button type="button" class:active={tab === "overview"} onclick={() => switchTab("overview")}>Overview</button>
      <button type="button" class:active={tab === "chat"} onclick={() => switchTab("chat")}>Chat</button>
      <button type="button" class:active={tab === "sessions"} onclick={() => switchTab("sessions")}>Sessions</button>
      <button type="button" class:active={tab === "memory"} onclick={() => switchTab("memory")}>Memory</button>
      <button type="button" class:active={tab === "cron"} onclick={() => switchTab("cron")}>Cron</button>
      <button type="button" class:active={tab === "tools"} onclick={() => switchTab("tools")}>Tools</button>
      <button type="button" class:active={tab === "settings"} onclick={() => switchTab("settings")}>Settings</button>
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
      <label class:model-attention={modelPickerAttention}>
        Model
        <select
          bind:this={modelSelectEl}
          bind:value={selectedModel}
          disabled={models.length === 0}
        >
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

  {#if tab === "overview"}
    <section class="panel overview">
      <div class="panel-head">
        <p class="hint">Gateway health, runtime snapshot, and cost summary.</p>
        <button type="button" onclick={loadOverview} disabled={dashboardLoading}>
          {dashboardLoading ? "Refreshing…" : "Refresh"}
        </button>
      </div>
      {#if dashboardLoading && !dashboardView}
        <p class="hint">Loading dashboard…</p>
      {:else if dashboardView}
        <div class="overview-grid">
          <div class="stat-card">
            <h2>Status</h2>
            <span class="stat ok">{dashboardView.status}</span>
            {#if dashboardView.paired !== null}
              <p class="hint">Paired: {dashboardView.paired ? "yes" : "no"}</p>
            {/if}
          </div>
          {#if dashboardView.hasCost}
            <div class="stat-card">
              <h2>Session Cost</h2>
              <span class="stat">{formatUsd(dashboardView.sessionCostUsd)}</span>
            </div>
            <div class="stat-card">
              <h2>Daily Cost</h2>
              <span class="stat">{formatUsd(dashboardView.dailyCostUsd)}</span>
            </div>
            <div class="stat-card">
              <h2>Monthly Cost</h2>
              <span class="stat">{formatUsd(dashboardView.monthlyCostUsd)}</span>
            </div>
            <div class="stat-card">
              <h2>Total Tokens</h2>
              <span class="stat">{formatInt(dashboardView.totalTokens)}</span>
            </div>
            <div class="stat-card">
              <h2>Requests</h2>
              <span class="stat">{formatInt(dashboardView.requestCount)}</span>
            </div>
          {/if}
        </div>
        <div class="stat-card runtime-card">
          <h2>Execution</h2>
          {#if dashboardView.executionSummary}
            <p class="hint">{dashboardView.executionSummary}</p>
          {:else}
            <p class="hint">No execution summary from gateway.</p>
          {/if}
        </div>
        <div class="stat-card runtime-card">
          <h2>Runtime</h2>
          <pre>{dashboardView.runtimeJson}</pre>
        </div>
      {:else}
        <p class="hint">Could not load dashboard.</p>
        <button type="button" onclick={loadOverview}>Retry</button>
      {/if}
    </section>
  {:else if tab === "chat"}
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
                <span class="session-name">{s.title}</span>
                <span class="session-meta">{formatSessionMeta(s)}</span>
              </button>
              <button type="button" class="session-del" onclick={() => removeSession(s.id)} title="Delete">×</button>
            </li>
          {:else}
            <li class="empty">No sessions — save token and create one.</li>
          {/each}
        </ul>
      </aside>

      <div class="chat-main">
        <div class="messages" bind:this={listEl}>
          {#each messages as msg}
            <article class={msg.role} class:step-fail={msg.role === "step" && msg.stepOk === false}>
              <div class="role">{msg.role}</div>
              {#if msg.role === "assistant"}
                <div class="body md">{@html renderMarkdown(msg.content)}</div>
              {:else if msg.role === "step" && msg.expand}
                <div class="body">
                  <details>
                    <summary>
                      <span class="step-cap">{msg.content}</span>
                      <span class="step-more" aria-hidden="true"></span>
                    </summary>
                    <pre class="step-expand">{msg.expand}</pre>
                  </details>
                </div>
              {:else}
                <div class="body">{msg.content}</div>
              {/if}
            </article>
          {/each}
          {#if streaming && !messages.some((m) => m.role === "status" || m.role === "step")}
            <p class="typing">Working…</p>
          {/if}
        </div>

        <footer>
          {#if !boundedDagLive}
          <div class="phase-row">
            <label>
              <input type="radio" name="host-phase" value="plan" bind:group={hostPhase} disabled={streaming} />
              Plan
            </label>
            <label>
              <input type="radio" name="host-phase" value="build" bind:group={hostPhase} disabled={streaming} />
              Build
            </label>
            <span class="hint">Plan blocks mutating tools (same as CLI --plan).</span>
          </div>
          {/if}
          {#if boundedDagLive && liveDag}
            <div class="dag-rail" aria-label="Task DAG">
              <div class="dag-rail-head">
                <strong>{liveDag.dag_id}</strong>
                {#if liveDag.fallback}
                  <span class="dag-fallback">fallback graph</span>
                {/if}
              </div>
              <ol class="dag-nodes">
                {#each liveDag.nodes as n}
                  <li class={`dag-node dag-${n.status}`} title={`${n.label} (${n.task_type}${n.contact ? ` · ${n.contact}` : ""})`}>
                    <span class="dag-id">{n.label}</span>
                    <span class="dag-caps">{n.contact ? n.contact : n.task_type}{n.caps ? ` · ${n.caps}` : ""}</span>
                  </li>
                {/each}
              </ol>
            </div>
          {/if}
          <div class="composer-row">
          <textarea
            bind:this={inputEl}
            class="composer"
            rows="4"
            bind:value={input}
            onkeydown={onKeydown}
            placeholder={boundedDagLive
              ? "Describe a task… Send plans the steps and runs them immediately."
              : "Message… (Enter to send, Shift+Enter for newline)"}
            disabled={streaming}
          ></textarea>
          {#if streaming}
            <button type="button" class="stop" onclick={stopStreaming} title="Stop">Stop</button>
          {:else}
            <button type="button" onclick={send} disabled={!input.trim()}>Send</button>
          {/if}
          </div>
        </footer>
      </div>
    </div>
  {:else if tab === "sessions"}
    <section class="panel sessions-panel">
      <div class="panel-head">
        <p class="hint">List, resume, or delete chat sessions. Each session keeps an isolated message history.</p>
        <button type="button" onclick={newSessionAndOpen}>+ New session</button>
        <button type="button" onclick={loadSessions}>Refresh</button>
      </div>
      {#if !token}
        <p class="hint">Save a bearer token to load sessions.</p>
      {:else}
        <ul class="session-list">
          {#each sessions as s}
            <li class:active={s.id === activeSessionId}>
              <div class="session-info">
                <strong>{s.title}</strong>
                <span class="session-meta">{formatSessionMeta(s)}</span>
              </div>
              <div class="session-actions">
                <button type="button" onclick={() => openSessionInChat(s.id)}>Open in Chat</button>
                <button type="button" class="danger" onclick={() => removeSession(s.id)}>Delete</button>
              </div>
            </li>
          {:else}
            <li class="empty">No sessions yet.</li>
          {/each}
        </ul>
      {/if}
    </section>
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
      <p class="hint">
        Runtime tool catalog from the gateway. Common ops use <code>shell</code> (git/gh/ssh via
        allowlist) and <code>git_operations</code>; there is no separate <code>gh</code> tool.
      </p>
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
      {#if routingDiagnostics?.panelVisible}
        <div class="diagnostics-panel">
          <strong>Capability routing diagnostics</strong>
          <p class="hint">Read-only summary when <code>[agent].intent_capability_route</code> is enabled. No secrets are shown.</p>
          <pre>{formatRoutingSummary(routingDiagnostics)}</pre>
          <p class="hint">Run locally for full explain output:</p>
          <ul class="doctor-commands">
            {#each routingDiagnostics.doctorCommands as cmd}
              <li><code>{cmd}</code></li>
            {/each}
          </ul>
        </div>
      {:else}
        <p class="hint diagnostics-off">
          Routing diagnostics are hidden by default. Enable
          <code>[agent].intent_capability_route</code> in <code>config.toml</code>, or run
          <code>velaclaw doctor routing</code> in a terminal.
        </p>
      {/if}
    </section>
  {/if}

  {#if pendingApprovals.length > 0}
    <div class="modal-backdrop" role="presentation">
      <div class="modal" role="dialog" aria-labelledby="approval-title">
        <h2 id="approval-title">
          {pendingApprovals[0].elevation
            ? "Elevate this command?"
            : "Tool approval required"}
        </h2>
        {#if pendingApprovals.length > 1}
          <p class="hint">{pendingApprovals.length} approvals queued — showing the oldest first.</p>
        {/if}
        <p><strong>{pendingApprovals[0].tool_name}</strong></p>
        <pre class="approval-args">{pendingApprovals[0].arguments_summary}</pre>
        <div class="modal-actions">
          <button type="button" class="danger" onclick={() => handleApproval(false)}>Deny</button>
          <button type="button" onclick={() => handleApproval(true)}>Allow once</button>
          <button type="button" onclick={() => handleApproval(true, true)}>Always allow</button>
          <button type="button" class="danger" onclick={() => handleApproval(false, false, true)}>Never</button>
        </div>
      </div>
    </div>
  {/if}

  {#if pendingHumanInput}
    <div class="modal-backdrop" role="presentation">
      <div class="modal" role="dialog" aria-labelledby="human-input-title">
        <h2 id="human-input-title">
          {#if pendingHumanInput.kind === "secret"}
            Enter secret
          {:else if pendingHumanInput.kind === "choice"}
            Choose an option
          {:else if pendingHumanInput.kind === "text"}
            Short code
          {:else}
            Confirm external step
          {/if}
        </h2>
        <pre class="approval-args">{pendingHumanInput.prompt}</pre>
        {#if pendingHumanInput.risk_note}
          <p class="risk-note">{pendingHumanInput.risk_note}</p>
        {/if}

        {#if pendingHumanInput.kind === "choice"}
          <div class="modal-actions stacked">
            {#each pendingHumanInput.options as opt}
              <button type="button" onclick={() => handleHumanInputChoice(opt)}>{opt}</button>
            {/each}
            <button type="button" class="danger" onclick={handleHumanInputCancel}>Cancel</button>
          </div>
        {:else if pendingHumanInput.kind === "text"}
          <p class="hint">Short values only (pairing code / id). Not for command output.</p>
          <label class="modal-field">
            Short response
            <input
              type="text"
              bind:value={humanInputText}
              autocomplete="off"
              maxlength="128"
              placeholder="e.g. pairing code"
            />
          </label>
          <div class="modal-actions">
            <button type="button" class="danger" onclick={handleHumanInputCancel}>Cancel</button>
            <button type="button" onclick={handleHumanInputTextSubmit}>Submit</button>
          </div>
        {:else if pendingHumanInput.kind === "secret"}
          <p class="risk-note">
            Sent only to this local daemon into a one-shot slot; never shown to the model. After
            submit, the agent continues with tools (e.g. sudo via secret_slot) — you should not run
            the command yourself.
          </p>
          <label class="modal-field">
            Password / token
            <input type="password" bind:value={humanInputSecret} autocomplete="off" />
          </label>
          <div class="modal-actions">
            <button type="button" class="danger" onclick={handleHumanInputCancel}>Cancel</button>
            <button type="button" onclick={handleHumanInputSecretSubmit}>Submit secret</button>
          </div>
        {:else}
          <p class="hint">
            Rare off-machine steps only. Machine work should use tool approval so the agent runs
            the command. Cancel to send the agent back to tools.
          </p>
          <div class="modal-actions">
            <button type="button" class="danger" onclick={handleHumanInputCancel}>Cancel</button>
            <button type="button" onclick={handleHumanInputHandoffDone}>Confirm external step</button>
          </div>
        {/if}
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
    height: 100%;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
    flex-shrink: 0;
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
  .overview-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }
  .stat-card {
    background: #0f172a;
    border-radius: 8px;
    padding: 0.75rem;
  }
  .stat-card h2 {
    margin: 0 0 0.35rem;
    font-size: 0.8rem;
    color: #94a3b8;
    font-weight: 500;
  }
  .stat {
    font-size: 1.35rem;
    font-weight: 600;
    color: #38bdf8;
  }
  .stat.ok {
    color: #4ade80;
  }
  .runtime-card pre {
    margin: 0;
    overflow-x: auto;
    font-size: 0.8rem;
    white-space: pre-wrap;
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
    flex-shrink: 0;
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
    min-height: 0;
    overflow: hidden;
  }
  .sessions {
    background: #1e293b;
    border-radius: 8px;
    padding: 0.5rem;
    overflow-y: auto;
    min-height: 0;
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
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .session-name {
    font-weight: 500;
    color: #e2e8f0;
  }
  .session-meta {
    font-size: 0.65rem;
    color: #94a3b8;
    line-height: 1.2;
  }
  .session-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .session-list li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.75rem;
    padding: 0.75rem;
    background: #0f172a;
    border-radius: 8px;
  }
  .session-list li.active {
    outline: 1px solid #0284c7;
  }
  .session-info {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }
  .session-actions {
    display: flex;
    gap: 0.35rem;
    flex-shrink: 0;
  }
  .session-actions button.danger {
    background: #7f1d1d;
  }
  .sessions-panel .panel-head {
    align-items: center;
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
    flex: 1;
    overflow: hidden;
  }
  .messages {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    min-height: 0;
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
  article.system {
    align-self: stretch;
    max-width: 100%;
    background: #451a03;
    border: 1px solid #f59e0b;
    color: #fde68a;
  }
  article.system .role {
    color: #fbbf24;
  }
  .role {
    font-size: 0.65rem;
    text-transform: uppercase;
    color: #94a3b8;
    margin-bottom: 0.25rem;
  }
  .body {
    line-height: 1.55;
    overflow-wrap: anywhere;
  }
  .body.md :global(p) {
    margin: 0.4rem 0;
  }
  .body.md :global(p:first-child) {
    margin-top: 0;
  }
  .body.md :global(p:last-child) {
    margin-bottom: 0;
  }
  .body.md :global(h1),
  .body.md :global(h2),
  .body.md :global(h3),
  .body.md :global(h4) {
    margin: 0.85rem 0 0.4rem;
    line-height: 1.3;
    font-weight: 650;
    color: #f8fafc;
  }
  .body.md :global(h1) {
    font-size: 1.25rem;
  }
  .body.md :global(h2) {
    font-size: 1.1rem;
  }
  .body.md :global(h3),
  .body.md :global(h4) {
    font-size: 1rem;
  }
  .body.md :global(ul),
  .body.md :global(ol) {
    margin: 0.4rem 0;
    padding-left: 1.35rem;
  }
  .body.md :global(li) {
    margin: 0.2rem 0;
  }
  .body.md :global(li > ul),
  .body.md :global(li > ol) {
    margin: 0.15rem 0;
  }
  .body.md :global(blockquote) {
    margin: 0.5rem 0;
    padding: 0.25rem 0.75rem;
    border-left: 3px solid #64748b;
    color: #cbd5e1;
  }
  .body.md :global(hr) {
    border: 0;
    border-top: 1px solid #475569;
    margin: 0.75rem 0;
  }
  .body.md :global(a) {
    color: #7dd3fc;
  }
  .body.md :global(table) {
    display: block;
    width: 100%;
    max-width: 100%;
    overflow-x: auto;
    border-collapse: collapse;
    margin: 0.6rem 0;
    font-size: 0.9rem;
  }
  .body.md :global(th),
  .body.md :global(td) {
    border: 1px solid #475569;
    padding: 0.35rem 0.55rem;
    text-align: left;
    vertical-align: top;
  }
  .body.md :global(th) {
    background: #1e293b;
    font-weight: 600;
    white-space: nowrap;
  }
  .body.md :global(tr:nth-child(even) td) {
    background: rgba(15, 23, 42, 0.35);
  }
  .body.md :global(pre) {
    margin: 0.5rem 0;
    padding: 0.65rem 0.75rem;
    overflow-x: auto;
    background: #0f172a;
    border-radius: 6px;
    font-size: 0.85rem;
  }
  .body.md :global(pre code) {
    background: transparent;
    padding: 0;
    border-radius: 0;
  }
  .body :global(code) {
    background: #0f172a;
    padding: 0.1rem 0.3rem;
    border-radius: 4px;
    font-size: 0.9em;
  }
  footer {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    align-items: stretch;
    flex-shrink: 0;
  }
  .composer-row {
    display: flex;
    gap: 0.5rem;
    align-items: stretch;
  }
  .phase-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem;
  }
  .phase-row label {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.85rem;
    color: #cbd5e1;
  }
  .dag-rail {
    width: 100%;
    background: #1e293b;
    border: 1px solid #334155;
    border-radius: 8px;
    padding: 0.6rem 0.75rem;
    margin: 0.35rem 0 0.5rem;
  }
  .dag-rail-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.5rem;
    margin-bottom: 0.4rem;
  }
  .dag-fallback {
    font-size: 0.75rem;
    color: #fbbf24;
  }
  .dag-nodes {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .dag-nodes li {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    position: relative;
    overflow: hidden;
    background: #0f172a;
    border: 1px solid #334155;
    border-radius: 6px;
    padding: 0.35rem 0.5rem;
    min-width: 7rem;
  }
  .dag-nodes li > * {
    position: relative;
    z-index: 1;
  }
  .dag-nodes li::before {
    content: "";
    position: absolute;
    inset: 0;
    width: 0;
    background: rgba(59, 130, 246, 0.28);
    transition: width 0.35s ease;
  }
  .dag-nodes li.dag-pending {
    opacity: 0.72;
  }
  .dag-nodes li.dag-running {
    border-color: #3b82f6;
  }
  .dag-nodes li.dag-running::before {
    width: 70%;
    animation: dag-node-pulse 1.1s ease-in-out infinite;
  }
  .dag-nodes li.dag-ok {
    border-color: #22c55e;
  }
  .dag-nodes li.dag-ok::before {
    width: 100%;
    background: rgba(34, 197, 94, 0.28);
  }
  .dag-nodes li.dag-error {
    border-color: #ef4444;
  }
  .dag-nodes li.dag-error::before {
    width: 100%;
    background: rgba(239, 68, 68, 0.32);
  }
  @keyframes dag-node-pulse {
    0%,
    100% {
      width: 35%;
      opacity: 0.55;
    }
    50% {
      width: 85%;
      opacity: 1;
    }
  }
  .dag-id {
    font-family: monospace;
    font-weight: 600;
    font-size: 0.85rem;
  }
  .dag-caps {
    font-size: 0.7rem;
    color: #94a3b8;
  }
  textarea {
    flex: 1;
    resize: vertical;
  }
  textarea.composer {
    min-height: 5.5rem;
    line-height: 1.45;
  }

  article.status .role,
  article.step .role {
    text-transform: lowercase;
  }
  article.status {
    opacity: 0.75;
    font-size: 0.85rem;
  }
  article.status .body {
    color: #94a3b8;
    font-style: italic;
  }
  article.step .body {
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 0.82rem;
    background: #0f172a;
    padding: 0.4rem 0.5rem;
    border-radius: 6px;
    white-space: pre-wrap;
    color: #7dd3fc;
  }
  article.step-fail .body {
    color: #fda4af;
  }
  article.step details summary {
    cursor: pointer;
    display: flex;
    align-items: baseline;
    gap: 0.55rem;
    list-style: disclosure-closed;
  }
  article.step details[open] summary {
    list-style: disclosure-open;
  }
  article.step .step-more {
    color: #64748b;
    font-size: 0.75rem;
    text-transform: lowercase;
  }
  article.step .step-more::after {
    content: "expand";
  }
  article.step details[open] .step-more::after {
    content: "collapse";
  }
  article.step pre.step-expand {
    margin: 0.45rem 0 0;
    white-space: pre-wrap;
    color: #cbd5e1;
    font-size: 0.78rem;
  }
  button.stop {
    background: #be123c;
    min-width: 4.5rem;
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
    min-height: 0;
    overflow-y: auto;
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
  .diagnostics-panel {
    margin-top: 0.75rem;
    padding: 0.75rem;
    background: #0f172a;
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .diagnostics-panel pre {
    margin: 0;
    font-size: 0.8rem;
    white-space: pre-wrap;
  }
  .doctor-commands {
    margin: 0;
    padding-left: 1.25rem;
    font-size: 0.8rem;
  }
  .diagnostics-off code {
    font-size: 0.75rem;
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
  .modal-actions.stacked {
    flex-direction: column;
    align-items: stretch;
  }
  .modal-field {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    margin-top: 0.75rem;
    font-size: 0.85rem;
  }
  .modal-field input {
    padding: 0.5rem 0.65rem;
    border-radius: 6px;
    border: 1px solid #334155;
    background: #0f172a;
    color: inherit;
  }
  .risk-note {
    margin-top: 0.75rem;
    color: #fbbf24;
    font-size: 0.85rem;
    line-height: 1.4;
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
    white-space: pre-wrap;
  }
  label.model-attention select {
    outline: 2px solid #f59e0b;
    box-shadow: 0 0 0 3px rgba(245, 158, 11, 0.35);
    animation: model-pulse 1.2s ease-in-out 3;
  }
  @keyframes model-pulse {
    0%,
    100% {
      box-shadow: 0 0 0 3px rgba(245, 158, 11, 0.35);
    }
    50% {
      box-shadow: 0 0 0 6px rgba(245, 158, 11, 0.15);
    }
  }
  @media (max-width: 720px) {
    .chat-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
