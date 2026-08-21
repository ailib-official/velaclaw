# VelaClaw

> Rust AI agent runtime — BYOK direct or Prism-routed, protocol-driven.

[![Crates.io](https://img.shields.io/crates/v/velaclaw)](https://crates.io/crates/velaclaw)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.87+-orange.svg)](https://www.rust-lang.org/)

[English](README.md) · [简体中文](README.zh-CN.md)

> **fr/ja/ru/vi translations are not yet synced with this version. Contributions welcome.**

---

## What is VelaClaw

VelaClaw is an autonomous AI agent runtime for Rust — the **ai-lib reference application** (capability routing + context envelope + a single tool loop). Providers are defined as [ai-protocol](https://github.com/ailib-official/ai-protocol) YAML manifests — zero hardcoded provider logic. WASM/MCP loaders are **not** the default product path.

**Two execution modes:**

| Mode | Path | Use when |
|------|------|----------|
| **BYOK** (default) | AiClient → provider API directly | You bring your own API keys |
| **Prism-routed** | Embedded prism-core router → provider | Multi-provider fallback, unknown models, usage telemetry |

VelaClaw is a **reference implementation** of a Rust agent on the ai-lib stack. It is not the only way — use `ai-lib-rust`, `ai-lib-python`, or `ai-lib-ts` to build your own.

---

## Install

```bash
# From crates.io
cargo install velaclaw

# Requires ai-protocol manifests
git clone https://github.com/ailib-official/ai-protocol ~/.velaclaw/ai-protocol
```

**From source:**

```bash
git clone https://github.com/ailib-official/velaclaw
cd velaclaw
cargo build --release
```

**MSRV**: Rust 1.87+

---

## Quick Start

### 1. Setup

```bash
velaclaw onboard
```

Walks through workspace initialization, API keys, and default model. Creates `~/.velaclaw/config.toml`.

Then validate (do **not** turn off the OS sandbox to “make it smooth”):

```bash
velaclaw doctor
```

Expect `envelope_assemble=true`, `contact host_decide=false` (live select is opt-in), `sandbox=landlock` or `fail-closed` on Linux, and `escape_on_approval=false`. `autonomy.level=full` does **not** disable the sandbox. Shell errors may use `[sandbox_deny]` / `[policy_deny]` — copy files into the workspace instead of retrying or setting `backend = "none"`.

### 2. Chat

```bash
# BYOK mode (default)
velaclaw agent -m "Explain Rust ownership in one paragraph"

# Specific model
velaclaw agent -m "What is WASM?" --model openai/gpt-4o-mini

# Prism-routed mode (requires prism-core + provider config)
velaclaw agent -m "Hello" --provider-mode prism
```

### 3. Web Chat UI

```bash
velaclaw daemon
# Open http://127.0.0.1:8080/chat
# Pair: POST http://127.0.0.1:8080/pair with header X-Pairing-Code: <code>
# Save bearer token in the UI; use Sessions tab or ?session=<id> to resume
```

Local Control REST/WebSocket routes are documented in [docs/commands-reference.md](docs/commands-reference.md#gateway--daemon). **Do not expose `/chat` on the public internet** without additional auth/TLS policy.

---

## CLI Commands

| Command | Description |
|---------|-------------|
| `velaclaw agent` | Interactive or one-shot chat |
| `velaclaw daemon` | Gateway: REST API + WebSocket + Chat SPA at `/chat` |
| `velaclaw onboard` | Interactive setup wizard |
| `velaclaw doctor` | Diagnostics and health checks |
| `velaclaw service` | Manage daemon lifecycle |
| `velaclaw cron` | Scheduled task engine |
| `velaclaw channel` | Telegram, Matrix, Lark, Discord |
| `velaclaw skills` | Extensible skill and tool system |
| `velaclaw memory` | Conversation storage (SQLite, PostgreSQL, Markdown) |
| `velaclaw config` | Configuration management |
| `velaclaw deploy` | Remote SSH deployment (`--features remote-deploy`) |
| `velaclaw hardware` | GPIO and peripherals (`--features hardware`) |

Run `velaclaw --help` for the full command tree.

---

## Configuration

### Environment

```bash
export AI_PROTOCOL_DIR=~/.velaclaw/ai-protocol
export OPENAI_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
export DEEPSEEK_API_KEY=sk-...
```

### `~/.velaclaw/config.toml`

```toml
[routing]
provider_mode = "byok"   # "byok" (default) or "prism"

[telemetry]
enabled = false          # BYOK usage reporting to Prism
endpoint = "https://api.prism.ailib.info"
```

---

## Feature Flags

**Default build** (`cargo build`) enables `ai-protocol` + `prism-router`.

| Flag | Description |
|------|-------------|
| `ai-protocol` *(default)* | Protocol-driven providers via ai-lib-rust |
| `prism-router` *(default)* | In-process prism-core routing for unknown providers |
| `channel-matrix` | Matrix with E2EE |
| `channel-lark` | Lark / Feishu |
| `browser-native` | Rust-native browser automation (fantoccini) |
| `hardware` | GPIO, serial peripherals |
| `peripheral-rpi` | Raspberry Pi GPIO (rppal) |
| `remote-deploy` | SSH-based remote deployment |
| `memory-postgres` | PostgreSQL memory backend |
| `observability-otel` | OpenTelemetry metrics |
| `sandbox-landlock` | Linux Landlock sandboxing (**on by default**) |
| `whatsapp-web` | Native WhatsApp Web client |
| `probe` | probe-rs for Nucleo memory |
| `rag-pdf` | PDF ingestion for RAG |

---

## Architecture

```
velaclaw
├── agent      BYOK (AiClient → provider) | Prism (prism-core router)
├── gateway    REST + WebSocket + /chat SPA
├── channels   Telegram, Matrix, Lark, Discord
├── memory     SQLite (default), PostgreSQL, Markdown
├── skills     Shell, file, browser, MCP, custom tools
├── cron       Scheduled task engine
├── telemetry  Optional BYOK usage → Prism (VL-EVO-003)
└── deploy     Optional remote SSH deploy
```

**Key decisions** (VL-ARCH-001):
- BYOK uses `AiClient` directly — keys never leave your machine
- `prism-core-routing` embedded as Cargo dependency (VL-EVO-002)
- Python/TS agents use their own runtimes, not VelaClaw (VL-ARCH-001 D7)

---

## Build Profiles

| Profile | When |
|---------|------|
| `release` | Production: max LTO, size-optimized, stripped |
| `release-fast` | Quick builds on 16GB+ RAM (8 codegen units) |
| `dev` | Development: fast compile, debug symbols |

```bash
cargo build --profile release-fast
cargo build --release                         # max optimization
cargo build --release --target aarch64-unknown-linux-gnu  # RPi cross-compile
```

---

## Dependencies

- [ai-lib-rust](https://crates.io/crates/ai-lib-rust) — AI client runtime
- [ai-protocol](https://github.com/ailib-official/ai-protocol) — Provider YAML manifests
- [prism-core-routing](https://crates.io/crates/prism-core-routing) — Embedded routing (VL-EVO-002)

Tracks upstream [VelaClaw Labs](https://github.com/VelaClaw-Labs/velaclaw) via `sync-upstream.sh`.

---

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
