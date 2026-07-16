# VelaClaw ↔ ai-lib-rust / ai-protocol migration

English summary + 中文：本页固定 **版本矩阵** 与本地开发方式（协议迁移 Phase 0）。

**User-facing migration** from built-in HTTP shorthands to `provider/model` + `AI_PROTOCOL_DIR`: see **`docs/migration-legacy-to-protocol.md`**.

**Clone / Git remote hygiene:** canonical development remote is **`ailib-official/velaclaw`**; do not leave `main` tracking an archived organization default (see **`CONTRIBUTING.md`** § “Canonical Git remote”).

## Compatibility window (Phase 6)

VelaClaw is **pre-1.0**; treat minors as potentially breaking until 1.0.

| Area | Policy |
|------|--------|
| `ai-lib-rust` (crates.io; ZS-ML-012) | Prefer published **1.1.0+** when available; VelaClaw currently pins git `rev = "ca5b605…"` (see Version matrix). Run `cargo test --features ai-protocol --locked` after any bump; BYOK credential chain uses `ai_lib_rust::credentials`. Local debugging may use `[patch.crates-io]` toward a git checkout — do not ship that patch in canonical releases unless policy explicitly allows it. |
| `ai-protocol` (Git) | Pin a **tag or commit** for reproducible QA; document the pin in your team runbook. Between tags, expect manifest schema drift — re-run protocol smoke tests when moving pins. |
| VelaClaw releases | Until 1.0, follow `CHANGELOG.md` [Unreleased]. ZS-ML-015 removed `legacy-providers`; chat providers now use `ai-protocol` manifests only. |

## Version matrix (pin for reproducible builds)

| Component | Recommended | Notes |
|-----------|-------------|--------|
| `ai-lib-rust` (git pin) | **`ca5b605`** (1.1.0 + CR-L3-001/002 `AssemblePool` + F5 identity; ALR-PROTO-PIN-003) | Cargo.toml `rev` in root + `velaclaw-config` + `velaclaw-agent-runtime`. |
| `ai-protocol` (Git) | tag or commit documented in team runbook | Manifest YAML + JSON Schema; set `AI_PROTOCOL_DIR` to a checkout root. |

Previous pin: `2124d467` (CR-L1 Envelope). Earlier: `6203a842` (chat endpoint fallback). Earlier: `8cfd647` / `4dd60c77`.

Patch bumps (0.9.x / 1.1.x) should stay semver-compatible within the declared major.minor; re-run `cargo test --features ai-protocol` after any bump.

## Environment

| Variable | Purpose |
|----------|---------|
| `AI_PROTOCOL_DIR` | Root of an `ai-protocol` clone (contains provider manifests / schema). Required for manifest-driven `AiClient` resolution at runtime. |

Optional: `AI_PROTOCOL_PATH` is recognized by some ai-lib tooling as an alias—prefer `AI_PROTOCOL_DIR` for VelaClaw docs consistency.

## BYOK credential availability

VelaClaw does not maintain a separate provider credential table for the protocol path. Provider availability shown by `velaclaw models protocol-providers` is delegated to ai-lib-rust's unified credential chain:

1. explicit application credential, when a caller supplies one to ai-lib;
2. the active manifest auth block (`endpoint.auth` first, then top-level `auth` for V1 compatibility);
3. conventional provider env fallback such as `OPENAI_API_KEY`;
4. native keyring support when enabled by ai-lib-rust.

The CLI reports only env var names and availability metadata. It must never print raw BYOK values.

## Local development with a git checkout

`Cargo.toml` (workspace root):

```toml
[patch.crates-io]
ai-lib-rust = { path = "../ai-lib-rust/crates/ai-lib-rust" }
```

Use only for local debugging; do not commit `[patch.crates-io]` unless the team explicitly maintains a fork workflow.

## Build / CI commands

```bash
# Protocol provider graph (required in CI)
cargo check --features ai-protocol
cargo test --features ai-protocol

# Manifest-only (same as default features today: `ai-protocol` only)
cargo test -p velaclaw --no-default-features --features ai-protocol

# Routing compile gate
cargo check -p velaclaw --features "ai-protocol routing_mvp" --lib
```

Feature flags:

- **`ai-protocol`** — enables optional `ai-lib-rust`, `protocol_registry`, and protocol CLI. **On by default.**
- **Legacy HTTP providers removed (ZS-ML-015)** — built-in string-key adapters (`openrouter`, `anthropic`, `custom:`, …) are no longer compiled by VelaClaw. Add or update ai-protocol manifests instead.
- **`routing_mvp`** — forwards `ai-lib-rust`’s experimental routing feature (optional). **Off by default.** Enable with `--features "ai-protocol routing_mvp"` when you need that code path; CI runs `cargo check -p velaclaw --features "ai-protocol routing_mvp" --lib` to prevent bitrot. **Metrics:** if `AiClient` exposes a metrics API in a future `ai-lib-rust` release, wire it to your observability layer without duplicating transport retry counters already covered here vs `[reliability]`.

### Deferred ai-lib-rust feature decisions (ZS-ML-009)

VelaClaw currently uses `ai-lib-rust` for chat and streaming only. The optional
`ai-lib-rust` features `embeddings`, `batch`, and `telemetry` are intentionally
**not enabled** in `Cargo.toml` until VelaClaw has concrete callers for them.

| ai-lib-rust feature | Decision | Rationale |
|---------------------|----------|-----------|
| `embeddings` | Deferred / removed from dependency features | No VelaClaw embedding path currently calls ai-lib; enabling it would add dependency weight without runtime value. |
| `batch` | Deferred / removed from dependency features | No batch API surface is wired in VelaClaw. |
| `telemetry` | Deferred / removed from dependency features | VelaClaw’s existing telemetry path is `observability-otel`; ai-lib metrics must be wired deliberately later to avoid duplicate counters. |

When adding any of these paths later, introduce a dedicated VelaClaw feature,
document the OpenTelemetry / metrics boundary, and add focused tests before
turning on the corresponding `ai-lib-rust` feature.

## CLI: manifest introspection

With `AI_PROTOCOL_DIR` set to a **local** ai-protocol checkout:

```bash
velaclaw models protocol-providers
velaclaw models protocol-models
velaclaw models protocol-providers --json
```

## Config: logical provider / model ids (Phase 2)

Manifest-backed chat uses the same **string shape** everywhere: `default_provider` is `manifest_provider_id/logical_model_id` (examples: `openai/gpt-4o-mini`, `anthropic/claude-3-5-sonnet-20241022`). Keys under `[reliability]` use the same grammar: `fallback_providers` lists alternate **provider ids** or full `provider/model` strings; `model_fallbacks` maps a primary **model id** string to an ordered list of fallback model ids (logical names from manifests).

Set API keys the way your manifests expect (e.g. `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or placeholders documented in ai-protocol). `AI_PROTOCOL_DIR` must point at the checkout whose YAML defines those provider/model entries.

**Minimal `config.toml` excerpt (copy-paste — adjust paths and keys):**

```toml
# Logical default: protocol provider + model (requires AI_PROTOCOL_DIR + credentials)
default_provider = "openai/gpt-4o-mini"
default_model = "gpt-4o-mini"

[reliability]
# After retries, try another logical route (same string grammar as default_provider)
fallback_providers = [
  "anthropic/claude-3-5-sonnet-20241022",
  "openai/gpt-4o",
]

# When this primary model errors, try these alternatives in order
[reliability.model_fallbacks]
"gpt-4o" = ["openai/gpt-4o-mini", "anthropic/claude-3-5-sonnet-20241022"]
```

## Resilience boundaries (Phase 4)

VelaClaw layers **two** independent mechanisms; keep them from overlapping in confusing ways:

| Layer | What it does | Where |
|-------|----------------|--------|
| **Transport retry** | `ai-lib-rust` returns `Error::is_retryable` / `retry_after` → limited retries inside `ProtocolBackedProvider` (`execute_chat_with_retry`). | `src/providers/protocol_adapter.rs` |
| **App failover** | `ReliableProvider` switches to another **provider name** or per-model alternatives from config after repeated failures. | `[reliability]` → `fallback_providers`, `model_fallbacks` |

**Guidance**

- Prefer **one** layer to own a given failure class: e.g. let ai-lib handle 429 backoff for a single logical model; use `fallback_providers` when you truly want a different backend (another provider id or `custom:` URL).
- Optional ai-lib features such as **`routing_mvp`** or future **`AiClient::metrics()`** integration are not required for the manifest path; enable deliberately when you add routing or SLO dashboards. As of ZS-ML-009, ai-lib `telemetry` is not enabled and does not feed VelaClaw’s OpenTelemetry pipeline.

## Next steps

- `docs/migration-legacy-to-protocol.md` — legacy shorthands, `AI_PROTOCOL_DIR`, and build/test matrix.
- Migration phases and PR sequencing are maintainer-owned; use this repo’s public docs (`docs/ai-lib-migration.md`, `docs/migration-legacy-to-protocol.md`) as the contributor source of truth.
