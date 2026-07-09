# Policy & Approval Runtime Reference

Canonical runtime contract for VelaClaw **0.7.0+** unified policy and approval layers (VL-SEC-001..006).

Related:

- Config keys: [config-reference.md](config-reference.md) — `[autonomy]`, L2/L2.5 policy files, channel `approval_mode`
- Migration from pre-0.7 behavior: [migration-policy-v0.7.0.md](migration-policy-v0.7.0.md)
- Channel setup: [channels-reference.md](channels-reference.md)

## Policy layers

| Layer | Location | Purpose |
|---|---|---|
| **L0** | ai-protocol manifest | `tool_calling` (parser, native strategy) |
| **L1** | `~/.velaclaw/config.toml` | `[autonomy]`, `[agent]`, `[gateway]`, `[security.*]` |
| **L2** | `<workspace>/agent-policy.yaml` | Workspace overrides: `tool_calling`, `autonomy`, `approval`, `self_adjust` (v1 or v2) |
| **L2.5** | `<workspace>/.velaclaw/policy-overrides.yaml` | Persistent operator/agent patches (session allowlist, autonomy tweaks) |
| **L3** | Session / channel profile | Per-sender dispatcher override, channel `approval_mode`, gateway pairing |

Merge order for autonomy/approval effective values:

**L1 config.toml** → **L2 agent-policy.yaml** → **L2.5 policy-overrides.yaml** → runtime session state.

## ApprovalGate (single human gate)

All supervised tool execution paths use `ApprovalGate`:

1. **Policy check** — `SecurityPolicy` / `PolicyHandle` (paths, shell risk, rate limits).
2. **Human check** — channel-specific backend (`CLI` stdin, `Gateway` Web UI hub, `Channel` inline prompt).

The shell tool schema **does not** expose an `approved` parameter. Human consent is injected internally after gate approval; models cannot self-approve.

### Three entry approval matrix

| Entry | Supervised tool approval | Shell medium-risk confirmation | Notes |
|---|---|---|---|
| **CLI** (`velaclaw agent`, one-shot) | Interactive stdin: `[Y]es / [N]o / [A]lways` | Same prompt path when policy requires human approval | `Always` persists tool to L2.5 `approval.session_allowlist` |
| **Gateway** (Web UI) | `ApprovalHub` modal / async request | Gateway hub prompt when shell policy requires it | Requires pairing when `require_pairing = true` |
| **Channel** (Telegram, Discord, …) | Controlled by `approval_mode` (see below) | Inline mode only; `deny` blocks interactive approval | Default: `inline` with timeout (300s) |

### Channel `approval_mode`

Set on each channel table, for example `[channels_config.telegram]`:

| Value | Behavior |
|---|---|
| `inline` (default) | Prompt in-channel (inline keyboard / Y-N-A). Supervised tools wait for human response. |
| `deny` | Deny any tool call that would require interactive approval. |
| `gateway_redirect` | Reserved; defer to gateway Web UI (not wired for all channels). |

## Tool batch execution

`run_tool_call_loop` and channel/gateway handlers call `execute_tool_batch()` (VL-UR-003):

- Multiple independent tool calls run **in parallel** when no pending call needs approval gating.
- When any call in the batch needs approval, the batch runs **sequentially** through the gate.
- Result order remains stable (matches call order).

`Agent::turn()` uses a separate path for the embedded web agent API; CLI/channel/gateway share the unified batch helper.

## L2 — `agent-policy.yaml` v2

Supported `version`: `1` or `2`. Version `2` adds autonomy and approval override sections:

```yaml
version: 2
tool_calling:
  dispatcher: auto
autonomy:
  level: supervised
  allowed_commands: [git, cargo, rg]
approval:
  auto_approve: [file_read, memory_recall]
  always_ask: [shell]
self_adjust:
  allowed_writes:
    - autonomy.allowed_commands
    - approval.session_allowlist
  denied_writes:
    - security.*
    - channels.*.credentials
    - gateway.paired_tokens
```

Discovery: project root or `workspace/agent-policy.yaml`, walking up from CWD; honors `VELACLAW_WORKSPACE`.

## L2.5 — `.velaclaw/policy-overrides.yaml`

User-facing persistent layer under the workspace:

```
<workspace>/.velaclaw/policy-overrides.yaml
```

Written by:

- Operator **Always** responses (appends to `approval.session_allowlist`)
- `policy_patch` tool when `self_adjust` allows the dot-path (requires `ai-protocol` feature)

Example:

```yaml
version: 1
approval:
  session_allowlist:
    - file_write
autonomy:
  allowed_commands:
    - git
    - cargo
```

After autonomy-related patches, `PolicyHandle` hot-refreshes in-process policy without restart.

## `policy_patch` tool

Available with `--features ai-protocol`. Applies validated dot-path patches to L2.5. Paths must match `self_adjust.allowed_writes` globs and must not match `denied_writes`.

Supported paths include:

- `approval.session_allowlist`
- `autonomy.level`, `autonomy.workspace_only`, `autonomy.allowed_commands`, `autonomy.forbidden_paths`
- `autonomy.auto_approve`, `autonomy.always_ask`

Denied by default: `security.*`, credential fields, `gateway.paired_tokens`.

## Defaults vs schema

Documented defaults match `AutonomyConfig` / schema defaults in `src/config/schema.rs`:

- `level = supervised`
- `workspace_only = true`
- `max_actions_per_hour = 100`
- `require_approval_for_medium_risk = true`
- `block_high_risk_commands = true`
- Channel `approval_mode = inline`
- Channel `approval_timeout_secs = 300`

## Audit

When `[security.audit]` is enabled, tool approval decisions are appended to the security audit log in addition to the in-memory `ApprovalManager` audit trail.
