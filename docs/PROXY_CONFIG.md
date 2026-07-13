# VelaClaw Proxy Configuration

> VLC-TRN-001 — `[proxy]` scope vs LLM (`ai-lib-rust`) egress (2026-07-08)

## Two proxy planes

VelaClaw has **two independent** HTTP proxy configurations:

| Plane | What it controls | Configuration |
|-------|------------------|---------------|
| **VelaClaw runtime** | Channels, tools, tunnel, memory, and other clients built via `build_runtime_proxy_client()` | `[proxy]` in `config.toml` + `proxy_config` tool |
| **LLM provider API** | BYOK / Prism-routed model calls via `ai-lib-rust` `HttpTransport` | Process env: `http_proxy`, `https_proxy`, `no_proxy`; optional `AI_PROXY_URL` failover |

`ExecutionHandle::from_config()` does **not** pass `[proxy]` into `AiClient`. This is intentional after [ALR-TRN-001](https://github.com/ailib-official/ai-lib-rust): LLM traffic follows standard system proxy env vars (same as curl/git/npm).

## Operator guidance

### LLM calls need a proxy

Set environment variables **before** starting `velaclaw` (or use `[proxy]` scope `environment` + `apply_env` to export them):

```bash
export https_proxy=http://127.0.0.1:7890
export http_proxy=http://127.0.0.1:7890
export no_proxy=localhost,127.0.0.1
velaclaw agent ...
```

Use `no_proxy` to bypass the proxy for specific hosts (e.g. `api.deepseek.com` when the provider is reachable directly).

Optional explicit ai-lib override (failover route, not a replacement for system env):

```bash
export AI_PROXY_URL=http://127.0.0.1:7890
```

### Channels / tools only

Use `[proxy]` with scope `velaclaw` or `services` — see [proxy-agent-playbook.md](./proxy-agent-playbook.md).

### Export `[proxy]` to the whole process (including LLM)

Use `proxy_config` action `apply_env` (scope `environment`) so `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` are set in the process. LLM calls will then pick them up via ai-lib-rust `auto_sys_proxy`.

**Do not** enable both conflicting per-request proxies: if `[proxy]` sets a client-level proxy for internals and you also export different env vars, behavior is defined per plane above.

## Decision record (VLC-TRN-001-R2)

**Option A (accepted):** No `config.toml` → `AiClient` wiring. Document boundaries; operators use system env (or `apply_env`) for LLM egress. `[proxy]` remains for VelaClaw-owned HTTP clients.

Option B (config → AiClient) deferred — risks double-proxy and priority ambiguity.

## References

- Cross-runtime policy: [ai-protocol `docs/TRANSPORT_PROXY_POLICY.md`](https://github.com/ailib-official/ai-protocol/blob/main/docs/TRANSPORT_PROXY_POLICY.md)
- Playbook: [proxy-agent-playbook.md](./proxy-agent-playbook.md)
- Decision id: `VLC-TRN-001` (maintainer record; Option A accepted above)
