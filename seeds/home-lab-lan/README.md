# home-lab-lan seed (example)

Example overlay for a LAN-oriented personal assistant workspace.

**Replace** host aliases, IPs, and paths to match your environment before use.

| File | Injected? | Notes |
|------|-----------|--------|
| `AGENTS.md` | yes | Thin posture |
| `SOUL.md` | yes | Persona + complete `<tool_call>` contract |
| `TOOLS.md` | yes | SSH / `gh` habits |
| `IDENTITY.md` | yes | Generic labels — edit name/role |
| `USER.md` | yes | Prefs + **compact host table** (critical) |
| `MEMORY.md` | yes | Keep empty / durable facts only |
| `INFRA.md` | **no** (today) | Longer host notes; facts also mirrored in `USER.md` |
| `config.snippet.toml` | n/a | Merge into `config.toml` (limits / allowlist) |

Contract: [`../../docs/workspace-prompt-templates.md`](../../docs/workspace-prompt-templates.md)
