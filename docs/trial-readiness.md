# VelaClaw Trial Readiness (VL-TRIAL-001)

End-to-end checklist for live provider testing with BYOK credentials.

## Prerequisites

1. **Merged PRs**
   - `ailib-official/velaclaw`: model-name fix in `protocol_adapter.rs`
   - `ailib-official/ai-protocol`: DeepSeek V4 model IDs + `dist/v2/providers/nvidia.json`

2. **Build**

```bash
cd velaclaw
cargo build --release -p velaclaw
```

3. **Environment**

```bash
export AI_PROTOCOL_DIR=/path/to/ai-protocol   # local checkout with updated dist/
export AI_PROXY_URL=http://192.168.2.13:8887  # ai-lib-rust reads this, not http_proxy
export DEEPSEEK_API_KEY=...
export NVIDIA_API_KEY=...
export OPENAI_API_KEY=...   # optional
```

For non-interactive shells (cron, daemons), put key exports in `~/.profile` or a
`~/.env` file sourced from both login and interactive shells — `~/.bashrc` alone may
not run in non-interactive contexts.

4. **Config**

```bash
velaclaw onboard
cp dev/config.trial.toml.example ~/.velaclaw/config.toml   # adjust paths
```

## Smoke tests

```bash
# Providers (v2 manifest discovery — nvidia should appear after ai-protocol PR)
velaclaw providers
velaclaw models list --protocol

# Single-turn chat
velaclaw agent -m "hello"

# NVIDIA
velaclaw agent -m "hello" --provider nvidia/moonshotai/kimi-k2-instruct

# Streaming (if supported by your CLI flags)
velaclaw agent -m "hello" --stream
```

## Known-good curl (proxy + DeepSeek V4)

```bash
curl -s -x http://192.168.2.13:8887 \
  https://api.deepseek.com/v1/chat/completions \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-v4-pro","messages":[{"role":"user","content":"hi"}],"max_tokens":20}'
```

## Task tracking

See `ai-lib-plans/active/projects/velaclaw/tasks/VL-TRIAL-001-trial-readiness.yaml`.
