# 第一章：快速入门

本章帮你从零开始，10 分钟内完成 VelaClaw 的安装和首次使用。

---

## 目录

1. [系统要求](#系统要求)
2. [安装方式](#安装方式)
3. [首次配置](#首次配置)
4. [第一次对话](#第一次对话)
5. [下一步](#下一步)

---

## 系统要求

### 最低配置

| 项目 | 要求 |
|------|------|
| 操作系统 | Windows 10+、macOS 10.15+、Linux |
| 内存 | 至少 512MB |
| 存储空间 | 至少 50MB |
| 网络 | 需要互联网连接（调用 AI API） |

### 推荐配置

| 项目 | 推荐 |
|------|------|
| 内存 | 2GB 或以上 |
| 存储 | 1GB |
| CPU | 任意现代处理器 |

---

## 安装方式

### 方式一：一键安装（推荐）

**Linux / macOS：**

```bash
curl -fsSL https://raw.githubusercontent.com/velaclaw-labs/velaclaw/main/bootstrap.sh | bash
```

**Windows（PowerShell 管理员模式）：**

```powershell
irm https://raw.githubusercontent.com/velaclaw-labs/velaclaw/main/bootstrap.ps1 | iex
```

### 方式二：手动安装

#### 步骤 1：安装 Rust

**Linux / macOS：**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Windows：**
1. 下载并安装 [Rustup](https://rustup.rs/)
2. 安装时选择默认选项

#### 步骤 2：验证安装

```bash
rustc --version
cargo --version
```

#### 步骤 3：安装 VelaClaw

```bash
# 克隆仓库
git clone https://github.com/velaclaw-labs/velaclaw.git
cd velaclaw

# 编译
cargo build --release

# 安装到系统
cargo install --path .
```

#### 步骤 4：验证 VelaClaw

```bash
velaclaw --version
```

---

## 首次配置

### 运行配置向导

```bash
velaclaw onboard
```

向导会问你以下问题：

#### 1. 选择 AI 提供商

```
请选择 AI 提供商：
1. OpenAI (推荐)
2. Anthropic (Claude)
3. OpenRouter (多模型聚合)
4. DeepSeek (国产)
5. 其他...

请输入数字选择: 3
```

**推荐选择**：
- **OpenRouter** - 一个账号访问多种模型，最灵活
- **OpenAI** - GPT 系列，稳定可靠
- **DeepSeek** - 国内用户友好，价格便宜

#### 2. 输入 API 密钥

```
请输入你的 API 密钥: sk-...
```

**如何获取 API 密钥？**

| 提供商 | 获取方式 |
|--------|----------|
| OpenAI | https://platform.openai.com/api-keys |
| Anthropic | https://console.anthropic.com/ |
| OpenRouter | https://openrouter.ai/keys |
| DeepSeek | https://platform.deepseek.com/ |

#### 3. 选择默认模型

```
请选择默认模型：
1. gpt-4o (强大但较贵)
2. gpt-4o-mini (平衡选择)
3. claude-sonnet (擅长编程)
4. deepseek-chat (经济实惠)

请输入数字选择: 2
```

#### 4. 选择记忆存储方式

```
请选择记忆存储方式：
1. SQLite (推荐) - 本地数据库，性能好
2. Markdown - 纯文本，方便查看
3. 无记忆 - 每次对话独立

请输入数字选择: 1
```

### 查看配置

配置文件位于 `~/.velaclaw/config.yaml`

查看当前配置：

```bash
cat ~/.velaclaw/config.yaml
```

---

## 第一次对话

### 方式一：命令行对话

```bash
# 单次对话
velaclaw agent --message "你好，介绍一下你自己"

# 进入交互模式
velaclaw agent
```

交互模式下：

```
你: 帮我写一个 Python 函数，计算斐波那契数列

VelaClaw: 好的！这是一个计算斐波那契数列的 Python 函数：

```python
def fibonacci(n):
    """计算斐波那契数列的第 n 项"""
    if n <= 1:
        return n
    return fibonacci(n-1) + fibonacci(n-2)

# 使用示例
for i in range(10):
    print(fibonacci(i))
```

你: 解释一下这段代码

VelaClaw: 这个函数使用递归方式计算斐波那契数列...
```

### 方式二：指定模型

```bash
# 使用特定模型
velaclaw agent --model gpt-4o --message "写一首关于春天的诗"
```

### 方式三：查看系统状态

```bash
velaclaw status
```

输出示例：

```
VelaClaw 状态
=============

版本: 0.1.0
运行模式: 正常
默认 Provider: openrouter
默认模型: openrouter/auto

记忆系统:
  后端: sqlite
  记录数: 42

认证状态:
  已配置: ✓
  密钥有效: ✓
```

---

## 常见安装问题

### 问题：编译失败

**原因**：缺少编译工具

**解决**：

```bash
# Ubuntu/Debian
sudo apt install build-essential

# Fedora
sudo dnf group install development-tools

# macOS
xcode-select --install
```

### 问题：找不到命令

**原因**：未添加到 PATH

**解决**：

```bash
# 添加到 PATH（添加到 ~/.bashrc 或 ~/.zshrc）
export PATH="$HOME/.cargo/bin:$PATH"

# 重新加载配置
source ~/.bashrc  # 或 source ~/.zshrc
```

### 问题：网络连接失败

**原因**：需要代理或网络问题

**解决**：

```bash
# 设置代理
export https_proxy=http://127.0.0.1:7890
export http_proxy=http://127.0.0.1:7890
```

---

## 下一步

恭喜！你已经成功安装并运行了 VelaClaw。

接下来可以：

1. **了解基础概念** → [基础概念](./02-basic-concepts.md)
2. **查看支持的模型** → [AI 模型与 Provider](./04-providers.md)
3. **接入 Telegram** → [Telegram 设置](./07-telegram.md)
4. **启用智能模型选择** → [智能模型选择](./05-smart-routing.md)

---

[← 返回目录](./README.md) | [下一章：基础概念 →](./02-basic-concepts.md)
