# Migrating from legacy string providers to `provider/model` + ai-protocol

This page is the **end-user** companion to `docs/ai-lib-migration.md`. It maps the **old
built-in HTTP factory** (short provider names, `custom:` URLs, and env-based API keys) to
the **protocol-first** model: logical ids like `openai/gpt-4o-mini` resolved through
`ai-lib-rust` + a local [ai-protocol](https://github.com/ailib-official/ai-protocol) clone.

## Glossary

| Term | Meaning |
|------|--------|
| **Legacy path** | Removed in ZS-ML-015: old string keys (`openrouter`, `anthropic`, `custom:…`) no longer compile into VelaClaw. |
| **Default path (today)** | `default = ["ai-protocol"]` in `Cargo.toml` — all model providers resolve through `ProtocolBackedProvider` and ai-protocol manifests. |
| **Protocol root** | Directory where YAML/JSON under `v2/providers` (or `dist/…`) describe providers and models. Set via **`AI_PROTOCOL_DIR`**. |
| **BYOK availability** | Whether ai-lib-rust can resolve a credential for the provider through the unified credential chain. VelaClaw reports the result but does not inspect or log secret values. |

## 1. Environment variables

| Variable | Role |
|----------|------|
| **`AI_PROTOCOL_DIR`** | **Required** for manifest-driven resolution: absolute or relative path to a **local** `ai-protocol` checkout (a directory, **not** an `http(s)://` URL). |
| `AI_PROTOCOL_PATH` | Backwards-compatible alias; same rules. Prefer `AI_PROTOCOL_DIR` in docs. |
| Vendor keys (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, …) | Unchanged: whatever your **manifests** and auth blocks expect. ai-lib-rust resolves `endpoint.auth`, top-level `auth`, conventional provider env fallbacks, and native keyring entries where enabled. |

If `AI_PROTOCOL_DIR` is wrong or points at a non-directory, `AiClient::new("provider/model")` fails. Quick setup prints a **yellow tip** when you choose a `provider/model` id but no usable local root is configured.

`velaclaw models protocol-providers` uses the same ai-lib credential resolver for its `available` column, so it catches both V2 `endpoint.auth.token_env` and conventional fallbacks such as `OPENAI_API_KEY`.

## 2. Shorthand: old provider name → new logical id

VelaClaw still accepts the **same string shape** in `default_provider` / `default_model` once you adopt manifests. Typical mappings (exact logical ids come from the YAML under your checkout):

| Legacy `provider` key (examples) | New style (illustrative) | Notes |
|----------------------------------|-------------------------|--------|
| `openai` | `openai/gpt-4o-mini` | Pick the model name from the manifest registry. |
| `openrouter` | e.g. `openrouter/…` | Depends on how manifests name the provider. |
| `ollama` / `qwen` / … | `provider_id/model_id` from manifests | If a vendor only existed in the old match arm, add/adapt manifests upstream. |
| `custom:https://api.example.com/v1` | A manifest-backed endpoint + logical model | Define the endpoint in **ai-protocol** so `AiClient` uses one code path. |

`custom:` and `anthropic-custom:` URL syntaxes now return a migration error. Create a provider
manifest for that endpoint instead.

Removal point: PR #32 (`bd15a62133445d6ef5a1c94ca6d59161c9c4ce08`) removed the
legacy HTTP factory. The last `main` commit before removal was PR #31
(`83ff308cd71c5aeb3c84fc6cd577243c094b6659`).

## 3. `Cargo` features (what to build)

| Command | Use when |
|---------|----------|
| `cargo test` / default features | **Protocol path** only — matches production default. |
| `cargo test --no-default-features --features ai-protocol` | **Manifest-only** build (no legacy factory). |
| `cargo check -p velaclaw --features "ai-protocol routing_mvp" --lib` | Routing compile gate. |

Current ai-lib-rust feature decision: VelaClaw does **not** enable ai-lib-rust
`embeddings`, `batch`, or `telemetry` features yet. Chat/streaming stays on the
protocol path; embeddings and batch APIs need explicit VelaClaw call sites
before enabling those dependency features. Telemetry continues through
`observability-otel`; ai-lib metrics should be wired in a future task only with a
documented no-duplicate-counter boundary.

## 4. CI / test matrix (Phase 7)

- **Default PR gate:** `ai-protocol` tests and `cargo check` (including `routing_mvp` compile gate as documented in `docs/ai-lib-migration.md`).
- **Removed:** legacy HTTP factory and `tests/provider_resolution.rs`.

## 5. Security

- **Never** put API keys in issue text, copy-pastable error dumps from production, or screenshot logs. Quick setup and CLI messages describe **env var names**, not values.
- Encrypted config (`enc2:`) behaviour is unchanged; this migration does not relax secret handling.

## 6. Related docs

- `docs/ai-lib-migration.md` — version matrix, feature flags, build commands.
- `CONTRIBUTING.md` — clone layout and `AI_PROTOCOL_DIR` for contributors.
- `active/projects/velaclaw/VELACLAW_AI_LIB_MIGRATION_PLAN.md` in **ai-lib-plans** — phases and PR list.

## 7. Embedded Prism routing (optional)

When `[routing] provider_mode = "prism"`, VelaClaw uses the in-process
`prism-core-routing` crate (not HTTP to a separate gateway). Configure Prism-managed
provider keys via environment variables such as `PRISM_GROQ_API_KEY`,
`PRISM_DEEPSEEK_API_KEY`, etc. Default `provider_mode = "byok"` keeps your own API keys
local and uses `ai-lib-rust` `AiClient` direct access.

```toml
[routing]
provider_mode = "prism"

default_model = "llama-3.1-8b-instant"
```

---

*ZS-ML-005 / Phase 5: legacy factory gating, migration story, and test matrix. ZS-ML-006 extends wizard
messages and the compatibility table in `docs/ai-lib-migration.md`.*
