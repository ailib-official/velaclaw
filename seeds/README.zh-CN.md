# Workspace seed 包

VelaClaw workspace 的示例 Markdown 叠加包。合同说明见：

- 中文：[`../docs/workspace-prompt-templates.zh-CN.md`](../docs/workspace-prompt-templates.zh-CN.md)
- English：[`../docs/workspace-prompt-templates.md`](../docs/workspace-prompt-templates.md)

## 包列表

| 包 | 用途 |
|----|------|
| [`home-lab-lan/`](home-lab-lan/) | 家庭实验室 / LAN 主机索引与工具习惯（示例叠加） |

## 手动应用（当前）

1. 复制到 agent workspace（默认 `~/.velaclaw/workspace/`）。
2. 按你的 `~/.ssh/config` 改主机名 / IP / 路径。
3. 可选：把 [`home-lab-lan/config.snippet.toml`](home-lab-lan/config.snippet.toml) 合并进 `config.toml`。
4. 重启 `velaclaw agent`（或新开会话）。

**勿**把密钥写进 seed。后续：`velaclaw onboard --seed <name>`。
