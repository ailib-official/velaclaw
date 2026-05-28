# VelaClaw 🕷️

**协议驱动的自主 AI Agent 运行时，支持智能模型选择和多模型协商。**

[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)

[English](README.md) · [简体中文](README.zh-CN.md)

[贡献指南](CONTRIBUTING.md) · [ai-lib 迁移（版本矩阵与环境）](docs/ai-lib-migration.md)

---

## 概述

VelaClaw 是一个 Rust 优先的自主 AI Agent 运行时，集成了 [ai-protocol](https://github.com/ailib-official/ai-protocol) 生态系统，实现智能的、协议驱动的 AI 操作。

### 核心特性

| 特性 | 描述 |
|------|------|
| **协议驱动 Provider** | 通过 YAML 文件配置 AI Provider，无需修改代码 |
| **智能模型选择** | 根据成本、速度、质量和可靠性自动选择最适合的模型 |
| **多模型协商** | 获取多个 AI 意见并综合出最佳响应 |
| **并行任务执行** | 同时执行独立任务，提高效率 |
| **远程部署** | 将 Agent 部署到远程服务器，支持受控访问 |
| **硬件集成** | 支持 GPIO、STM32 和其他外设 |

---

## 快速开始

### 前置条件

- Rust 1.87+（2021 版本；由 `rust-toolchain.toml` 锁定）
- 本地 [ai-protocol](https://github.com/ailib-official/ai-protocol) checkout，用于 Provider manifests

### 构建

```bash
# 克隆仓库
git clone https://github.com/ailib-official/velaclaw.git
cd velaclaw

# 使用默认协议支持构建
cargo build
```

### 树莓派交叉编译 (aarch64)

```bash
# 安装目标平台
rustup target add aarch64-unknown-linux-gnu

# 构建发布版本
cargo build --release --target aarch64-unknown-linux-gnu

# 二进制文件位置
ls target/aarch64-unknown-linux-gnu/release/velaclaw
```

### 运行

```bash
# 使用默认协议 provider/model 发送一条消息
cargo run -- agent -m "Hello"

# 指定具体的 provider/model id
cargo run -- agent -p openai/gpt-5.2 -m "Hello"

# 启用实验性多端点路由（ai-lib-rust/routing_mvp）。
# 这是构建期开关，无需额外 CLI 参数。
cargo build --features routing_mvp
```

---

## 配置

### 环境变量

```bash
# AI Protocol 目录（用于协议驱动的 Provider）
# 克隆自：git clone https://github.com/ailib-official/ai-protocol
export AI_PROTOCOL_DIR=/path/to/ai-protocol

# ai-protocol manifest 声明的 Provider API 密钥
export OPENAI_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
```

### 配置文件

```toml
# ~/.velaclaw/config.toml
default_provider = "openai/gpt-5.2"
default_model = "openai/gpt-5.2"
```

---

## 文档

- [用户指南](docs/user-guide.zh-CN.md) - 完整的用户文档
- [集成指南](docs/ai-protocol-integration-guide.zh-CN.md) - 开发者集成指南
- [上游需求](docs/upstream-requirements.zh-CN.md) - 对 ai-lib-rust 和 ai-protocol 的需求

### 用户指南章节

1. [入门指南](docs/user-guide/01-getting-started.md)
2. [基本概念](docs/user-guide/02-basic-concepts.md)
3. [与 AI 聊天](docs/user-guide/03-chat-with-ai.md)
4. [Provider 配置](docs/user-guide/04-providers.md)
5. [智能路由](docs/user-guide/05-smart-routing.md)
6. [通道配置](docs/user-guide/06-channels.md)
7. [Telegram 集成](docs/user-guide/07-telegram.md)
8. [工具使用](docs/user-guide/10-tools.md)
9. [多模型协商](docs/user-guide/13-negotiation.md)
10. [自动化任务](docs/user-guide/14-automation.md)
11. [远程部署](docs/user-guide/15-deployment.md)
12. [安全设置](docs/user-guide/17-security.md)
13. [命令参考](docs/user-guide/18-commands.md)
14. [配置参考](docs/user-guide/19-config.md)
15. [常见问题](docs/user-guide/20-faq.md)

---

## 功能标志

只有在 `Cargo.toml [features]` 中声明的标志才有效。`ai-protocol` 默认启用，其它皆为可选。

| 标志 | 分组 | 描述 |
|------|------|------|
| `ai-protocol`（默认） | AI | 通过 `ai-lib-rust` 的协议驱动 Provider（`provider/model` id，依赖 `AI_PROTOCOL_DIR`）。 |
| `routing_mvp` | AI | 通过 `ai-lib-rust/routing_mvp` 的实验性多端点路由。 |
| `channel-matrix` | 通道 | Matrix 客户端 + E2EE 解密（需要 matrix-sdk ≥ 0.16.1）。 |
| `channel-lark` | 通道 | 飞书/Lark WebSocket 长连接通道。 |
| `whatsapp-web` | 通道 | 原生 WhatsApp Web 客户端（wa-rs）。 |
| `hardware` | 硬件 | USB 设备发现 + 串口外设框架。 |
| `peripheral-rpi` | 硬件 | 树莓派 GPIO 外设（仅 Linux）。 |
| `probe` | 硬件 | `probe-rs` STM32/Nucleo 内存读取。 |
| `memory-postgres` | 存储 | 在默认 SQLite 之外启用 PostgreSQL 记忆后端。 |
| `rag-pdf` | 存储 | 用于数据手册 RAG 的 PDF 提取。 |
| `observability-otel` | 运维 | OTLP trace + metrics 导出。 |
| `remote-deploy` | 运维 | 通过 SSH 的受控远程部署。 |
| `browser-native` | 工具 | Rust 原生浏览器自动化（fantoccini）。 |
| `sandbox-landlock` | 安全 | 用于工具执行的 Linux Landlock 沙箱。 |

文案中提到的"智能模型选择"和"多模型协商"目前仍是路线图设计文档：[`docs/user-guide/05-smart-routing.md`](docs/user-guide/05-smart-routing.md) 与 [`docs/user-guide/13-negotiation.md`](docs/user-guide/13-negotiation.md)，**尚未**作为 Cargo feature 或 CLI 参数对外暴露。

## 仪表盘

运行网关（`velaclaw gateway`）时，可通过 `GET /dashboard` 访问监控仪表盘：

- **状态**：健康与配对状态
- **成本**：会话、日、月成本及 Token 用量（需在配置中启用 `[cost] enabled = true`）
- **运行时**：组件健康快照

仪表盘每 30 秒自动刷新。

---

## 架构

VelaClaw 使用 trait 驱动的模块化架构：

- **Providers**：AI 模型后端（OpenAI、Anthropic、本地模型等）
- **Channels**：通信平台（Telegram、Discord、Matrix 等）
- **Tools**：可扩展的工具执行（shell、文件、浏览器等）
- **Memory**：持久化存储后端（SQLite、PostgreSQL 等）
- **Security**：策略执行和密钥管理

---

## 上游依赖

VelaClaw 集成了：

- [ai-lib-rust](https://crates.io/crates/ai-lib-rust) - 协议驱动的 AI API 客户端（crates.io，使用 `--features ai-protocol` 启用）
- [ai-protocol](https://github.com/ailib-official/ai-protocol) - Provider YAML 配置（克隆后设置 `AI_PROTOCOL_DIR`）

### 同步上游更新

VelaClaw 追踪 [velaclaw-labs/velaclaw](https://github.com/velaclaw-labs/velaclaw) 的更新：

```bash
# 列出上游变更
./sync-upstream.sh --list

# 预览合并
./sync-upstream.sh --dry-run

# 执行合并
./sync-upstream.sh

# 选择性合并特定提交
./sync-upstream.sh --cherry-pick <commit-hash>
```

---

## 许可证

可选择以下任一许可证：

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

---

## 作者

**Luqiang Wang**

---

## 致谢

VelaClaw 是 ZeroSpider/ZeroClaw 项目重命名后的延续（重命名追踪号 ZS-RN-001 / ZS-RN-002，详见 `git log`）。上游规范仓库为 [`velaclaw-labs/velaclaw`](https://github.com/velaclaw-labs/velaclaw)；维护流程见 [同步上游更新](#同步上游更新)。

基于：
- [`ai-lib-rust`](https://crates.io/crates/ai-lib-rust) —— 协议驱动的 AI API 客户端（使用 `--features ai-protocol` 启用，默认开启）。
- [`ai-protocol`](https://github.com/ailib-official/ai-protocol) —— Provider manifest YAML 集合（克隆后设置 `AI_PROTOCOL_DIR`）。
