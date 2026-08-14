# Workspace 提示词模板（结构化 README 草案）

> **状态：** 面向运维与发行打包的结构化草案。  
> **尚非**正式终端用户手册——作为未来 User Guide 章节的种子文档。  
> **读者：** 运维、家庭实验室维护者、发行打包者。  
> **更新日期：** 2026-07-29。

## 1. 摘要

VelaClaw 的行为很大程度由 **文档驱动**：workspace 内短 Markdown 会注入 system prompt。准确、短小的索引，往往比整库治理文档或改代码更能纠正「人格 / 习惯 / 本地上下文」类问题。

| 层次 | 控制什么 | 常见位置 |
|------|----------|----------|
| **运行时代码** | 工具、SSH 执行、解析、安全策略 | VelaClaw 二进制 |
| **`config.toml`** | 限额、白名单、自治级别、Provider | `~/.velaclaw*/config.toml` |
| **Workspace Markdown** | 人格、工具习惯、主机索引、用户偏好 | `…/workspace/*.md` |

**本稿非目标：** 重做 onboard UX、在安装器里直接下发 seed 包、或立刻改代码注入列表（仅作后续项列出）。

## 2. 注入机制

每次 agent 会话会把 workspace 身份类文件（若存在且非空）注入 system prompt，包括：

| 当前会注入 | 作用 |
|------------|------|
| `AGENTS.md` | 工作姿态（简洁、少过程元评论） |
| `SOUL.md` | 人格、风格、工具格式硬规则 |
| `TOOLS.md` | 工具习惯（SSH vs 本地路径、GitHub/`gh` 等） |
| `IDENTITY.md` | 名称 / 角色标签 |
| `USER.md` | 用户偏好 + **短**环境索引（推荐放这里） |
| `HEARTBEAT.md` | 可选周期检查说明 |
| `BOOTSTRAP.md` | 首次运行 / 初始化说明 |
| `MEMORY.md` | 长期用户事实（保持精简） |

**重要：** 不在上表中的文件（例如自定义 `INFRA.md`）**不会**自动注入。关键事实应写入会注入的文件（通常是 `USER.md` / `TOOLS.md`），或让 agent `file_read`，或以后在代码中扩展注入列表。

`velaclaw onboard` 已会生成其中若干文件。把它们当作产品内置种子；站点专用包应 **叠加** 短索引，而不是用长篇小说替换整个 SOUL。

## 3. 推荐模板包（分层）

包要 **短**。优先「投影」（短索引），不要整库导入（私有规划仓、治理语料库等）。

```text
workspace/
  AGENTS.md      # 薄姿态
  SOUL.md        # 人格 + tool_call 契约 +「勿复读工具输出」
  TOOLS.md       # 习惯：SSH 主机、gh、可选 grep 提示
  IDENTITY.md    # 名称/角色
  USER.md        # 偏好 + 紧凑主机表（会注入！）
  MEMORY.md      # 仅长期事实
  INFRA.md       # 可选较长主机说明（默认不注入）
  _sources/      # 可选只读摘录（给人看 / 偶发 file_read）
```

### 3.1 分层职责

| 层 | 变更频率 | 内容规则 |
|----|----------|----------|
| 人格（`SOUL` / `AGENTS`） | 少 | 抽象规则；无密钥；不要堆主机 IP 清单 |
| 习惯（`TOOLS`） | 偶发 | 格式与 do/don’t；示例必须是完整 `<tool_call>…</tool_call>` |
| 用户偏好（`USER`） | 按人 | 语言、简洁、「工具已展示则只总结」 |
| 环境索引（`USER` 表和/或 `INFRA`） | 随基础设施变 | 主机 **别名**、角色、规范路径；**禁止**密码/PAT |
| 配置（`config.toml`） | 按安装 | `max_tool_iterations`、`max_actions_per_hour`、`allowed_commands`、自治级别 |

### 3.2 环境索引应写什么

应包含：

- SSH Host 别名（`piubt`、`lan-git`、`eos-hk`…）及其「**不是**本地目录」
- 角色一句话（代理 / 裸仓 SoT / VPS）
- 非显而易见的规范路径（如裸仓在 `/srv/git/repos`）
- 「这台机器**不是**什么」（例如 LAN git SoT 是裸仓 + SSH，不是 Gitea）

不应包含：

- 密码、PAT、私钥、token 提取步骤
- 私有规划仓 / 治理语料库 全文拷贝
- 长篇风险评估或臆测架构

## 4. 写作质量要求

1. **短优于全。** Prompt 预算有限；几十行索引胜过几百行。
2. **抽象规则放 SOUL；具体主机放 USER/INFRA。** 不要把一次性 e2e 脚本写进人格。
3. **工具调用必须完整。** 裸 JSON 或只剩 `</tool_call>` 会当聊天展示且 **不会执行**。
4. **不要复读步骤 caption。** 出现 `git status` / `read foo.rs` 一类短行后，助手只做简短归纳。
5. **空白 `Error:`** 常表示 exit≠0 且 stderr 为空（如 `grep` 无匹配）。可选搜索先判断存在或加 `|| true`。
6. **改完 workspace 文档或 `config.toml` 后重启 agent**（或新开会话），以便 CLI 重新加载。

## 5. 文档替代不了的配置项

Markdown **不能**提高限流。请在 `config.toml` 设置（家用 trial 曾用量级示例）：

| 键 | 作用 | 说明 |
|----|------|------|
| `[agent].max_tool_iterations` | 单轮工具循环深度 | 默认偏低（约 10）；多步运维需调高 |
| `[autonomy].max_actions_per_hour` | 动作预算 | 默认偏低（约 20） |
| `[autonomy].allowed_commands` | Shell 白名单 | `full` 会合并额外命令；仍可显式加入 `gh` / `jq` |
| `[autonomy].level` | 自治姿态 | 影响批准与白名单合并 |

密钥放在 OS 钥匙串 / 加密配置 / SSH agent——**永远不要**写进 Markdown 模板。

## 6. 发行用 seed 布局（后续）

仓库已包含可 **手动复制** 的示例包：

- [`seeds/README.zh-CN.md`](../seeds/README.zh-CN.md) — 包索引
- [`seeds/home-lab-lan/`](../seeds/home-lab-lan/) — LAN / 家庭实验室叠加（使用前请改主机）

未来发行可将本 README 视为更多 seed 包的 **合同**：

```text
seeds/
  default/           # 与 onboard 兼容的人格桩（未来）
  home-lab-lan/      # 示例叠加：USER 主机表 + TOOLS SSH/gh 习惯
  README.md
  README.zh-CN.md
```

建议合并策略（仅提案）：

1. 只补缺失文件；未经 `--force` / 确认不覆盖已定制的 `SOUL.md` / `USER.md`。
2. 叠加包可在 `USER.md` 缺少「本地基础设施」节时追加该节。
3. seed 内永不携带密钥。
4. 可选：`velaclaw onboard --seed <name>`（尚未实现）。

## 7. 验收清单

应用模板包后：

- [ ] 用 SSH 别名提一个口语化远程问题（不得把主机名当本地目录）。
- [ ] 确认出现简短步骤 caption（如 `git status`、`read file`），而不是 stdout 全文。
- [ ] 最终 `>>` 回复是短总结，不是第二份全文。
- [ ] `config.toml` 限额符合该安装档位。
- [ ] `workspace/` 下无密码/PAT。

## 8. 升级为 User Guide 的路径

正式推广时：

1. 叙事迁入 `docs/user-guide/`，配 onboard 与 workspace 文件截图。
2. 保留本文（或精简版）作为打包者的 **模板合同**。
3. 若多数安装需要独立主机索引，可考虑将 `INFRA.md` 加入运行时注入列表。
4. 在 `velaclaw onboard` / bootstrap 增加显式 `--seed <name>`。

## 9. 相关文档

- 入门导航：[getting-started/README.md](getting-started/README.md)
- 配置参考：[config-reference.md](config-reference.md)
- 策略与批准：[policy-approval-reference.md](policy-approval-reference.md)
- 命令参考：[commands-reference.md](commands-reference.md)

---

**文档类型：** README 草案 / 打包合同  
**下一步升级目标：** User Guide —「Workspace 人格与本地索引」
