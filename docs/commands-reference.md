# VelaClaw Commands Reference

This reference is derived from the current CLI surface (`velaclaw --help`).

Last verified: **July 30, 2026**.

## Top-Level Commands

| Command | Purpose |
|---|---|
| `onboard` | Initialize workspace/config quickly or interactively |
| `agent` | Run interactive chat or single-message mode |
| `gateway` | Start webhook and WhatsApp HTTP gateway |
| `daemon` | Start supervised runtime (gateway + channels + optional heartbeat/scheduler) |
| `service` | Manage user-level OS service lifecycle |
| `doctor` | Run diagnostics and freshness checks; includes config-vs-rebuild hints |
| `status` | Print current configuration and system summary |
| `cron` | Manage scheduled tasks |
| `models` | Refresh provider model catalogs |
| `providers` | List provider IDs, aliases, and active provider |
| `channel` | Manage channels and channel health checks |
| `integrations` | Inspect integration details |
| `skills` | List/install/remove skills |
| `migrate` | Import from external runtimes (currently OpenClaw) |
| `config` | Export machine-readable config schema |
| `completions` | Generate shell completion scripts to stdout |
| `hardware` | Discover and introspect USB hardware |
| `peripheral` | Configure and flash peripherals |

## Command Groups

### `onboard`

- `velaclaw onboard`
- `velaclaw onboard --interactive`
- `velaclaw onboard --channels-only`
- `velaclaw onboard --force`
- `velaclaw onboard --api-key <KEY> --provider <ID> --memory <sqlite|lucid|markdown|none>`
- `velaclaw onboard --api-key <KEY> --provider <ID> --model <MODEL_ID> --memory <sqlite|lucid|markdown|none>`
- `velaclaw onboard --api-key <KEY> --provider <ID> --model <MODEL_ID> --memory <sqlite|lucid|markdown|none> --force`

`onboard` safety behavior:

- If `config.toml` already exists, `onboard` asks for explicit confirmation before overwrite.
- In non-interactive environments, existing `config.toml` causes a safe refusal unless `--force` is passed.
- Use `velaclaw onboard --channels-only` when you only need to rotate channel tokens/allowlists.

### `agent`

- `velaclaw agent`
- `velaclaw agent -m "Hello"`
- `velaclaw agent --provider <ID> --model <MODEL> --temperature <0.0-2.0>`
- `velaclaw agent -p nvidia --model meta/llama-3.1-8b-instruct -m "pong"` — VL-RT-004: bare `-p` + vendor-qualified `--model` composes to `nvidia/meta/…` (do not pass `meta/…` alone)
- `velaclaw agent --peripheral <board:path>`
- `velaclaw agent --no-color`
- `velaclaw agent --no-fold`
- `velaclaw agent --plan -m "Propose a change"` — Plan phase: mutating tools are blocked (default is Build). With `[agent].bounded_dag_live = true` and empty `bounded_dag_path`, Plan runs a tool-free planner (session default) then prints the linear DAG; omit `--plan` (Build) to run each work node through the existing tool loop. Short Build approvals reuse the session DAG; other Build text replans. Same `host_phase` contract as Web chat.
- `velaclaw agent --session-id <id> -m "Continue"` — load/save `workspace/.velaclaw/chat_sessions`
- `velaclaw undo` — restore tracked files to HEAD if workspace already has `.git`

Interactive REPL (no `-m`) starts a **new memory session** each launch (VL-MEM-001). Conversation/Daily autosave is scoped to that session; prior sessions and legacy unscoped Conversation rows are not injected. Core (long-term) memories may still appear. `/new` / `/clear` clears this session's Conversation/Daily and rotates to another new session (Core preserved). Resume prior chat sessions in the browser via `/chat` (Sessions tab or sidebar; deep link `?session=<id>`).

BYOK default hygiene (VL-RT-003): if the configured `default_model` provider has no
usable env API key, the agent remaps to a keyed provider (or fails with an
actionable error) instead of calling the nvidia free-tier default without
`NVIDIA_API_KEY`. See [providers-reference.md](providers-reference.md#byok-default-model-hygiene-vl-rt-003).

NIM / multi-segment model ids (VL-RT-004): when `-p <provider>` is a bare provider
id and `--model` contains `/` whose first segment differs from that provider,
VelaClaw composes `provider/model` (e.g. `nvidia` + `meta/llama-…` →
`nvidia/meta/llama-…`). Full `--model nvidia/…` is unchanged. Details:
[providers-reference.md](providers-reference.md#nvidia-nim-notes).

Opt-in context Envelope (CR-L1/L2 / VL-CTX-001): `[agent].envelope_assemble` defaults to **`true`** (requires `--features ai-protocol`). All surfaces use `context_orch::prepare_turn_history` (optional LLM compact when over `max_history_messages`, then `assemble_layered`). Set `envelope_assemble = false` only as an emergency kill-switch (message-count trim). HardBudget fails the turn (channel replies with an error). See [config-reference.md](config-reference.md).

CR-L3-003 async schedule façade (opt-in, default off): set `[agent].envelope_assemble_async = true` **in addition to** `envelope_assemble = true` to use ai-lib `AssemblePool` (same assemble algorithm; bounded concurrency / timeout; fail-closed). Sync remains the default algorithm path.

Turn model selection (CLI + Web) shares `orchestration::resolve_turn_model`: explicit user pick → `host_decide` → `intent_capability_route` → `query_classification` / `default_model`. Channels still use `route.model` only. See the wiring matrix in [config-reference.md](config-reference.md#agent).

DAG flags (`template_dag`, `candidate_dag_shadow`, `candidate_dag_emit`) are **library/doctor** gates — they do **not** change live chat. Observe with doctor commands below.

`[agent].bounded_dag_live` is a **separate** opt-in (default `false`): CLI + Web only. Empty path = planner node (session default model) then linear work nodes; not L4 `candidate_dag_emit`. See [config-reference.md](config-reference.md#agent).

Opt-in template DAG shell (CR-L2 library): `agent::dag_runner` walks handwritten DAGs. The `[agent].template_dag` bool is reserved/unused on live turns. Observe with `velaclaw doctor template-dag --fixture <path>`.

CR-L4-002 library: `agent::candidate_dag::{validate_candidate_dag_json, run_candidate_or_fallback}` — schema/capability fail or run abort can fall back to a handwritten L2 template; optional output-hash stagnation via `TemplateRunOptions`.

CR-L4-004 structured logs (M3c/d/e): stable fields `m3c_pass`, `m3d_category`, `m3e_fallback` on events `candidate_dag_run` / `candidate_dag_fallback` / `candidate_dag_schema_fail` / `candidate_dag_shadow_run`. Logs only — no Prometheus/Grafana gate. See [l4-m3-metrics.md](l4-m3-metrics.md).

CR-L4-003 shadow host (default-off library/doctor): set `[agent].candidate_dag_shadow = true` to allow `maybe_run_candidate_shadow` for callers that invoke it. Live agent chat loop stays unchanged. Observe anytime with:

```bash
velaclaw doctor candidate-dag --candidate <path> [--fallback <path>] [--message <text>] [--compact] [--stagnation-limit N]
# Local M3 aggregate (CR-HOST-002; no Grafana):
RUST_LOG=info velaclaw doctor candidate-dag --candidate <path> 2>shadow.log
velaclaw doctor l4-shadow-summary --log shadow.log
```

CR-CAP-005 capability-index route (default-off; CAP-003 wire): set `[agent].intent_capability_route = true` (alias `capability_index_route`) to resolve turn models via **explicit Tag / Hint** → host capability index → **reachable (local keys)** ∩ `[[model_routes]]` on CLI + Web (after explicit pick / `host_decide`). NL `query_classification` is optional only. Empty reachable sets fail closed.

**Operator pipeline (CR-CAP-007)** — same story across doctor surfaces; live chat stays off unless the flag is true:

```bash
# 1) declared vs reachable facts
velaclaw doctor capabilities --tag coding
velaclaw doctor capabilities --tag coding --reachable-only

# 2) opt-in select observe (prefer explicit Tag; no NL required)
velaclaw doctor capability-route --tag coding --force
# alias: doctor intent-route …

# 3) BYOK/prism execution path after a model is chosen
velaclaw doctor routing
```

Pin a Tag for a turn without the classifier: pass `--tag <Tag>` to `capability-route` (doctor), or enable the config flag and use Hint/`[[model_routes]]` / explicit Tag wiring documented above. Do **not** treat NL intent classification as the product mainline.

```bash
velaclaw doctor intent-route [--message <text>] [--hint <hint>] [--tag <Tag>] [--force] [--persist] [--rebuild]
# alias: doctor capability-route …
```

Interactive REPL extras:

- `/expand <id>` — replay scrubbed tool output for a step caption from this session
- **Esc Esc** (TTY, within 500ms) — stop the current turn and return to the prompt (same classify/persist contract as Web)
- Web Chat: while a turn is running, **Stop** cancels it (same `CancellationToken` + [`cancel-contract.md`](cancel-contract.md))
- Long tool outputs fold after `[cli_render].fold_lines` (default `10`) when stdout is a TTY and `--no-fold` is not set
- `--no-color` forces plain output (also honors `NO_COLOR`); non-TTY/pipe already strips ANSI

See `[cli_render]` in [config-reference.md](config-reference.md).

### `gateway` / `daemon`

- `velaclaw gateway [--host <HOST>] [--port <PORT>]`
- `velaclaw daemon [--host <HOST>] [--port <PORT>]`

Both start the HTTP gateway (REST + WebSocket + embedded Web UI). `daemon` also supervises channels, optional heartbeat, and scheduler when configured.

#### Web Control UI (`/chat`)

After the gateway is listening (default `http://127.0.0.1:8080`):

1. Open **`GET /chat`** in a browser — Svelte SPA (Chat, Sessions, Memory, Cron, Tools, Settings).
2. If pairing is enabled, exchange the one-time startup code via **`POST /pair`** (header `X-Pairing-Code: <code>`) to obtain a bearer token.
3. Paste the token into the SPA toolbar and click **Save token**.
4. Resume a saved chat session with **`/chat?session=<session-id>`** (also persisted in browser local storage).

**Security (required reading):**

- Treat the Control UI as a **local management plane**, not a public chat product.
- **Do not** expose `/chat` or Local Control API routes on the public internet without additional fronting auth/TLS policy.
- When pairing is enabled, protected routes require `Authorization: Bearer <token>` (same as `/webhook`).
- Prefer loopback bind (`127.0.0.1`) for interactive use; non-loopback binds require explicit operator intent.

#### Local Control API (gateway HTTP surface)

| Route | Method | Auth | Purpose |
|---|---|---|---|
| `/health` | GET | Public | Liveness; `version` (= `velaclaw --version`); paired summary; runtime snapshot |
| `/metrics` | GET | Public | Prometheus metrics |
| `/dashboard` | GET | Public | Legacy monitoring HTML (cost/runtime) |
| `/api/dashboard` | GET | Public | JSON dashboard payload (health + optional cost) |
| `/chat` | GET | Public | Web Control UI (static SPA) |
| `/pair` | POST | Pairing code header | Exchange one-time code → bearer token |
| `/webhook` | POST | Bearer (when pairing on) | Simple prompt webhook |
| `/ws` | GET | Bearer query `?token=` (when pairing on) | Streaming chat WebSocket |
| `/api/chat` | POST | Bearer | Non-streaming chat completion (**no turn-cancel**; use `/ws` Stop) |
| `/api/providers` | GET | Bearer | BYOK provider/model availability |
| `/api/providers/{id}/test` | POST | Bearer | Provider connectivity probe |
| `/api/sessions` | GET, POST | Bearer | List / create chat sessions |
| `/api/sessions/{id}` | GET, DELETE | Bearer | Session detail / delete |
| `/api/memory` | GET | Bearer | Search/list memory entries |
| `/api/memory/{id}` | GET | Bearer | Single memory entry |
| `/api/config` | GET, PUT | Bearer | Read/write runtime config subset |
| `/api/config/schema` | GET | Bearer | Config schema for UI forms |
| `/api/cron` | GET, POST | Bearer | List / create cron jobs |
| `/api/cron/{id}` | GET, PUT, DELETE | Bearer | Cron job CRUD |
| `/api/cron/{id}/run` | POST | Bearer | Trigger cron job once |
| `/api/tools` | GET | Bearer | Tool catalog exposed to agent |
| `/api/approvals/{id}/respond` | POST | Bearer | Approve/deny pending tool execution |

Channel webhooks (`/whatsapp`, `/linq`, `/nextcloud-talk`) follow channel-specific verification; see [channels-reference.md](channels-reference.md).

Pairing flow example:

```bash
curl -sS -X POST http://127.0.0.1:8080/pair \
  -H 'X-Pairing-Code: 123456'
# → { "token": "...", "paired": true, ... }
```

See also [operations-runbook.md](operations-runbook.md) for daemon lifecycle and [troubleshooting.md](troubleshooting.md) for gateway failures.

### `service`

- `velaclaw service install`
- `velaclaw service start`
- `velaclaw service stop`
- `velaclaw service restart`
- `velaclaw service status`
- `velaclaw service uninstall`

### `doctor`

- `velaclaw doctor` — config, protocol, workspace, daemon, and environment checks (envelope assemble, Contact live-select flags, sandbox, autonomy Full ≠ no sandbox); ends with a short `[maintenance]` hint (config vs rebuild)
- `velaclaw doctor maintenance` — full operator guide (layers, hot-reload, preflight, when to rebuild) plus **VL-OPS-001** PATH/install binary hygiene (which `velaclaw` is first on PATH vs this process; warns on multiple known installs — observe-only)
- `velaclaw doctor l4-shadow-summary --log <path>|'-' [--json]` — **CR-HOST-002** local aggregate of L4 M3c/d/e fields (`m3c_pass` / `m3d_category` / `m3e_fallback`) from tracing or JSONL logs. Observe-only; does **not** enable `[agent].candidate_dag_shadow`; Prometheus/Grafana are **not** an entry gate. Tip: `RUST_LOG=info velaclaw doctor candidate-dag --candidate <path> 2>shadow.log` then summarize that file.
- `velaclaw doctor models [--provider <ID>] [--use-cache]`
- `velaclaw doctor template-dag --fixture <path> [--message <text>] [--compact]` — validate a handwritten CR-L2 template DAG JSON (walk + Envelope assemble only; no LLM). Fail-closed on `max_steps` / HardBudget / invalid graph. Requires `--features ai-protocol`. Independent of `[agent].template_dag` (that flag gates future runtime wiring; default remains `false`).
- `velaclaw doctor candidate-dag --candidate <path> [--fallback <path>] [--message <text>] [--compact] [--stagnation-limit N]` — CR-L4-003 shadow observe: validate candidate DAG + optional L2 fallback (assemble-only; no LLM). Independent of `[agent].candidate_dag_shadow` (default `false`). Requires `--features ai-protocol`.
- `velaclaw doctor capabilities [--tag <Tag>] [--rebuild] [--reachable-only]` — CR-CAP-002 / CR-CAP-004 / CR-HOST-001 / CR-CAP-007 host-local Tag→candidates inverted index over `$AI_PROTOCOL_DIR` (cache: `<config_dir>/capability-index.json`). Prints declared vs **reachable** counts (reachable = providers with a usable local API key / keyless local; query-time filter — **no secrets** in the cache). With `--tag`, each candidate is marked `[reachable]` or `[no-key]`; `--reachable-only` lists only keyed candidates; suggests `capability-route --tag … --force` next. Rebuild triggers: explicit `--rebuild`, protocol tip/root change, missing cache (**no** daily timer). Facts + reachability UX; live selection is CR-CAP-005 (`intent_capability_route`). Requires `--features ai-protocol`.
- `velaclaw doctor generative [--capability <key>] [--reachable-only] [--json]` — **Experimental (VL-GEN-002)**: list PT-GEN keys (`image_generation` / `speech_to_text` / `text_to_speech`) per `metadata.models` against local `AI_PROTOCOL_DIR`. Reuses the VL-GEN-001 inspect path (GOV-007). **reachable** = `allowed` ∧ local key presence (no secrets). Omitted capability keys and missing `endpoints.<key>` fail closed. Does **not** add those keys to CR-CAP `CAPABILITY_TAGS` / intent-route. Requires `--features ai-protocol`.
- `velaclaw doctor intent-route` / `capability-route` `[--message <text>] [--hint <hint>] [--tag <Tag>] [--rebuild] [--force] [--persist]` — CR-CAP-005 / CAP-003 wire / CR-HOST-001 / CR-CAP-007 observe: Tag/Hint → declared → **reachable** ∩ constraints; explainable decision (no LLM). Prefer `--tag`. Independent of `[agent].intent_capability_route` when `--force` is set. `--persist` appends JSONL to `<config_dir>/intent-route-decisions.jsonl`. Empty reachable sets fail closed. Requires `--features ai-protocol`.
- `velaclaw doctor host-decide [--message <text>] [--tag <Tag>] [--force] [--set-override provider/model] [--clear-override] [--session-key <id>]` — **ORCH-HOST-001/002** observe: CAP reachable ∩ host Decide (CostRouter pricing when available; stub otherwise; prints `used_cost_router`). Independent of `[agent].host_decide` when `--force` is set. Session override is process-local and must remain in reachable set. Requires `--features ai-protocol`.
- `velaclaw doctor dag-view --fixture <path> [--tag <Tag>] [--set-override provider/model] [--session-key <id>]` — **ORCH-DAG-VIS-001** render DAG JSON as a text view-model and list CAP reachable picker options. Rejects unreachable overrides. Node-level ModelSelector product UI is a non-goal (capabilities shown read-only). Requires `--features ai-protocol`.
- `velaclaw doctor dag-emit --candidate <path> [--fallback <path>] [--message <text>] [--compact] [--stagnation-limit N]` — **ORCH-DAG-EMIT-001** schema-strict candidate path (extract JSON → validate → L2 fallback). Observe-only; independent of `[agent].candidate_dag_emit` (default `false`). Live chat unchanged. Requires `--features ai-protocol`.
- `velaclaw doctor dag-plan [--message <text>] [--fallback <path>] [--force] [--compact] [--stagnation-limit N] [--temperature N]` — **ORCH-DAG-EMIT-002** call the configured planner model to generate DAG JSON, then validate → L2. Requires `[agent].candidate_dag_emit = true` or `--force`. **Not** the default `Agent::turn` chat path. Requires `--features ai-protocol`.
- `velaclaw doctor routing` — VL-DR-001 / OmniRoute L-V1 / CR-CAP-007: explain `routing.provider_mode`, configured vs BYOK-effective logical model (VL-RT-003 hygiene), detected credential env *names* only, and points at the capability-index doctor trio. Prism mode prints logical id + `PRISM_*_API_KEY` reminder. Requires `--features ai-protocol`.

See [config-externalization.md](config-externalization.md) for the canonical operator contract.

### `cron`

- `velaclaw cron list`
- `velaclaw cron add <expr> [--tz <IANA_TZ>] <command>`
- `velaclaw cron add-at <rfc3339_timestamp> <command>`
- `velaclaw cron add-every <every_ms> <command>`
- `velaclaw cron once <delay> <command>`
- `velaclaw cron remove <id>`
- `velaclaw cron pause <id>`
- `velaclaw cron resume <id>`

Notes:

- Mutating schedule/cron actions require `cron.enabled = true`.
- Shell command payloads for schedule creation (`create` / `add` / `once`) are validated by security command policy before job persistence.

### `models`

- `velaclaw models refresh`
- `velaclaw models refresh --provider <ID>`
- `velaclaw models refresh --force`
- `velaclaw models protocol-providers`
- `velaclaw models protocol-models`
- `velaclaw models protocol-generative --model <provider/model> --capability image_generation`

`models refresh` currently supports live catalog refresh for provider IDs: `openrouter`, `openai`, `anthropic`, `groq`, `mistral`, `deepseek`, `xai`, `together-ai`, `gemini`, `ollama`, `llamacpp`, `astrai`, `venice`, `fireworks`, `cohere`, `moonshot`, `glm`, `zai`, `qwen`, and `nvidia`.

`protocol-generative` inspects Experimental PT-GEN keys (`image_generation`, `speech_to_text`, `text_to_speech`) against local `AI_PROTOCOL_DIR` manifests. Omitted capability keys fail closed. It does not call vendor HTTP. For a catalog-style reachable view, use `velaclaw doctor generative`.

### `channel`

- `velaclaw channel list`
- `velaclaw channel start`
- `velaclaw channel doctor`
- `velaclaw channel bind-telegram <IDENTITY>`
- `velaclaw channel add <type> <json>`
- `velaclaw channel remove <name>`

Runtime in-chat commands (Telegram/Discord while channel server is running):

- `/models`
- `/models <provider>`
- `/model`
- `/model <model-id>`

Channel runtime also watches `config.toml` and hot-applies updates to:
- `default_provider`
- `default_model`
- `default_temperature`
- `api_key` / `api_url` (for the default provider)
- `reliability.*` provider retry settings
- `[agent].max_tool_iterations`

`add/remove` currently route you back to managed setup/manual config paths (not full declarative mutators yet).

### `integrations`

- `velaclaw integrations info <name>`

### `skills`

- `velaclaw skills list`
- `velaclaw skills install <source>`
- `velaclaw skills remove <name>`

`<source>` accepts git remotes (`https://...`, `http://...`, `ssh://...`, and `git@host:owner/repo.git`) or a local filesystem path.

Skill manifests (`SKILL.toml`) support `prompts` and `[[tools]]`; both are injected into the agent system prompt at runtime, so the model can follow skill instructions without manually reading skill files.

### `migrate`

- `velaclaw migrate openclaw [--source <path>] [--dry-run]`

### `config`

- `velaclaw config schema`

`config schema` prints a JSON Schema (draft 2020-12) for the full `config.toml` contract to stdout.

### `completions`

- `velaclaw completions bash`
- `velaclaw completions fish`
- `velaclaw completions zsh`
- `velaclaw completions powershell`
- `velaclaw completions elvish`

`completions` is stdout-only by design so scripts can be sourced directly without log/warning contamination.

### `hardware`

- `velaclaw hardware discover`
- `velaclaw hardware introspect <path>`
- `velaclaw hardware info [--chip <chip_name>]`

### `peripheral`

- `velaclaw peripheral list`
- `velaclaw peripheral add <board> <path>`
- `velaclaw peripheral flash [--port <serial_port>]`
- `velaclaw peripheral setup-uno-q [--host <ip_or_host>]`
- `velaclaw peripheral flash-nucleo`

## Validation Tip

To verify docs against your current binary quickly:

```bash
velaclaw --help
velaclaw <command> --help
```
