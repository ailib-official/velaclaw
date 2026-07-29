# Workspace seed packs

Example Markdown overlays for VelaClaw workspaces. See the contract:

- English: [`../docs/workspace-prompt-templates.md`](../docs/workspace-prompt-templates.md)
- 简体中文: [`../docs/workspace-prompt-templates.zh-CN.md`](../docs/workspace-prompt-templates.zh-CN.md)

## Packs

| Pack | Purpose |
|------|---------|
| [`home-lab-lan/`](home-lab-lan/) | Home-lab / LAN host index + tool habits (example overlay) |

## Apply (manual, for now)

1. Copy files into your agent workspace (default `~/.velaclaw/workspace/`).
2. Edit host names / IPs / paths to match **your** `~/.ssh/config`.
3. Optionally merge [`home-lab-lan/config.snippet.toml`](home-lab-lan/config.snippet.toml) into `config.toml`.
4. Restart `velaclaw agent` (or start a new session).

Do **not** commit secrets into these seeds. Future: `velaclaw onboard --seed <name>`.
