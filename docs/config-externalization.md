# Config & Policy Externalization (Operator Contract)

Last verified: **July 12, 2026**.

This page defines what operators can change **without rebuilding VelaClaw**, and what still requires a binary release. It supports the protocol-first model: manifests + `config.toml` + policy YAML drive runtime behavior; Rust code stays the execution engine.

## Layers (outside → inside)

| Layer | Location | Typical changes | Restart needed? |
|---|---|---|---|
| L3 Protocol | `$AI_PROTOCOL_DIR` provider/model manifests | New provider, endpoint aliases (`chat` / `chat_openai`), tool_calling | Restart process (or new CLI session); no rebuild |
| L1 Config | `~/.velaclaw/config.toml` (or workspace config) | Provider/model, agent limits, routes, reliability | See hot-reload below |
| L2 Policy | `agent-policy.yaml` | Tool dispatcher preference, autonomy overrides | Next turn / policy merge |
| L2.5 Overrides | `<workspace>/.velaclaw/policy-overrides.yaml` | Approval allowlist, autonomy patches | Autonomy hot via `PolicyHandle`; approval list on next session reload |

## Hot-reload (`velaclaw channel start`)

On each inbound channel message, VelaClaw re-reads `config.toml` when the file stamp changes and applies:

- `default_provider`
- `default_model`
- `default_temperature`
- `api_key` / `api_url`
- `reliability.*`
- `[agent].max_tool_iterations`

**Not** hot-reloaded today (restart required):

- `[routing].provider_mode` (`byok` vs `prism`) and execution-handle backend
- Channel credentials / enable flags (most channel tables)
- Feature-gated compile options (`routing_mvp`, optional channel crates, etc.)
- Upgrading the pinned `ai-lib-rust` dependency (new runtime parsers / endpoint fallbacks)

CLI one-shot (`velaclaw agent -m …`) always loads a fresh config for that process.

## Switching models (no rebuild)

Preferred order:

1. Edit `default_provider` + `default_model` in `config.toml`, **or**
2. Override with `VELACLAW_PROVIDER` / `VELACLAW_MODEL`, **or**
3. CLI: `velaclaw agent -p <provider> --model <model> -m "…"`

Ensure `$AI_PROTOCOL_DIR` points at a local [ai-protocol](https://github.com/ailib-official/ai-protocol) checkout that contains the provider manifest.

Preflight:

```bash
export AI_PROTOCOL_DIR=/path/to/ai-protocol
velaclaw doctor
velaclaw doctor maintenance   # built-in config-vs-rebuild guide
velaclaw models protocol-providers
```

`velaclaw doctor` ends with a short `[maintenance]` hint. `velaclaw doctor maintenance` prints the full operator guide (same contract as this page).

Doctor checks that the protocol root exists, the default provider is indexed, and the manifest exposes `endpoints.chat` or `endpoints.chat_openai`.

## Hint-based routing (config-only)

Use stable hints so call sites do not hard-code model ids:

```toml
[[model_routes]]
hint = "reasoning"
provider = "deepseek"
model = "deepseek-v4-flash"

[[model_routes]]
hint = "fast"
provider = "groq"
model = "llama-3.1-8b-instant"

[query_classification]
enabled = true

[[query_classification.rules]]
hint = "reasoning"
keywords = ["explain", "analyze", "why"]
```

Automatic multi-provider routing via `routing_mvp` / prism policy remains optional.
With `[[model_routes]]` + `[query_classification]` enabled, CLI / `Agent::turn` /
`process_message` resolve matching hints to `hint:<name>` for `RouterProvider`
(see VL-RT-002). Keep `reliability.fallback_providers` for failover.

Capability routing uses operator-facing **hints** (for example `reasoning`, `fast`)
configured in `[[model_routes]]` / `[query_classification]` above. Tag vocabulary
and maintainer ADR notes are not required for a public checkout — configure hints
here and keep manifests under `AI_PROTOCOL_DIR`.

Requires a rebuild when upgrading the pinned `ai-lib-rust` revision (VL-RT-001).

## When you must rebuild / release

- Runtime bugs or contract mismatches (CLI overrides, dispatcher wiring, security policy code)
- New Cargo features or dependency bumps (`ai-lib-rust`, `prism-core-routing`)
- New channel/tool crates that are compile-time optional

## Related

- [config-reference.md](config-reference.md) — key defaults
- [migration-legacy-to-protocol.md](migration-legacy-to-protocol.md) — protocol path
- [operations-runbook.md](operations-runbook.md) — day-2 ops
- [troubleshooting.md](troubleshooting.md) — failure matrix
