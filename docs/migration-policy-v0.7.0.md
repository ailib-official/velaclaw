# Migration — Unified Policy & Approval (0.7.0)

Guide for operators upgrading from **0.6.0** (CLI render release) or earlier **0.5.x** installs to **0.7.0** unified policy/approval behavior.

Runtime reference: [policy-approval-reference.md](policy-approval-reference.md)

## Summary of breaking / behavior changes

| Area | Before (≤0.6.0) | After (0.7.0) |
|---|---|---|
| Shell `approved` parameter | Model could pass `approved=true` in tool args | **Removed** — only `ApprovalGate` injects human approval |
| Channel supervised tools | Often ran with `approval=None` (auto-deny or bypass) | Channel `approval_mode` + inline/gateway gate |
| Session allowlist | Lost on restart | Persisted to `<workspace>/.velaclaw/policy-overrides.yaml` |
| Agent self-adjust | Schema only | `policy_patch` tool + `self_adjust` glob enforcement |
| Tool loop parallelism | Could skip approval in some paths | Gate-aware; parallel only when no approval needed |
| `Agent::execute_tool_call` success | Always reported `success: true` | Propagates actual tool outcome |

## Step 1 — Review `[autonomy]`

Recommended production defaults (unchanged intent, now enforced consistently):

```toml
[autonomy]
level = "supervised"
workspace_only = true
allowed_commands = ["git", "cargo", "rg"]  # required for shell
require_approval_for_medium_risk = true
block_high_risk_commands = true
```

If you previously relied on models setting `approved=true` for shell, expect **denials** until a human approves via CLI, gateway, or channel inline prompt.

## Step 2 — Configure channel approval

Add `approval_mode` to messaging channels under `[channels_config.*]`:

```toml
[channels_config.telegram]
bot_token = "..."
allowed_users = ["velaclaw_user"]
approval_mode = "inline"        # default; use "deny" to block supervised tools
approval_timeout_secs = 300
```

See [channels-reference.md](channels-reference.md#supervised-tool-approval-channel-profile).

## Step 3 — Optional workspace policy files

**L2 — `agent-policy.yaml`** (v2 recommended):

```yaml
version: 2
self_adjust:
  allowed_writes:
    - autonomy.allowed_commands
    - approval.session_allowlist
  denied_writes:
    - security.*
    - channels.*.credentials
```

**L2.5** is created automatically at `<workspace>/.velaclaw/policy-overrides.yaml` when operators choose **Always** or when the agent uses `policy_patch`.

## Step 4 — Remove stale prompts/docs

- Remove any system prompts instructing the model to set `approved: true`.
- Update runbooks that assumed channel tools bypass approval.

## Step 5 — Validate

1. CLI supervised shell → expect Y/N/A prompt.
2. Telegram inline → expect approval buttons before `shell` / gated tools.
3. Restart daemon → **Always**-approved tools remain in session allowlist via L2.5 file.
4. `cargo test` / `./dev/ci.sh all` in your deployment environment.

## Rollback

Revert to **0.6.0** tag/commit if unified approval blocks production workflows. L2.5 files are forward-compatible YAML; they are ignored by older versions that do not load them.
