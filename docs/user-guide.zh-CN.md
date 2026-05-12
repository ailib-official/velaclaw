# VelaClaw 智能助手 - 新功能使用指南

欢迎！本指南将帮助你了解 VelaClaw 最新版本的强大新功能。无需编程知识，跟着指南操作即可。

---

## 目录

1. [功能概览](#功能概览)
2. [智能模型选择](#智能模型选择)
3. [多专家协商](#多专家协商)
4. [远程部署](#远程部署)
5. [常见问题](#常见问题)

---

## 功能概览

### 这次更新带来了什么？

VelaClaw 现在变得更聪明了：

| 功能 | 简单解释 |
|------|----------|
| **智能模型选择** | 自动为你选择最合适的 AI 模型 |
| **多专家协商** | 多个 AI 同时回答，取最佳结果 |
| **并行处理** | 同时处理多个任务，速度更快 |
| **远程部署** | 可将 VelaClaw 部署到远程服务器 |

---

## 智能模型选择

### 什么是智能模型选择？

想象你有一个智能助手，每次提问时它会自动判断：

- 你问的是**编程问题**→ 选择最擅长编程的模型
- 你问的是**创意写作**→ 选择最擅长写作的模型
- 你问的是**翻译**→ 选择最擅长翻译的模型

**你不需要手动选择，VelaClaw 会自动帮你做决定。**

### 它是怎么工作的？

VelaClaw 会从以下维度评估每个 AI 模型：

| 评估维度 | 说明 |
|----------|------|
| **速度** | 响应快不快 |
| **成本** | 使用花费多少 |
| **可靠性** | 容不容易出错 |
| **质量** | 回答质量如何 |

然后根据你的问题类型，自动选择综合评分最高的模型。

### 如何启用？

在启动 VelaClaw 时添加 `--smart` 参数：

```bash
velaclaw --smart
```

或者在配置文件中设置：

```yaml
# ~/.velaclaw/config.yaml
routing:
  smart_selection: true
```

### 实际例子

**场景 1：编程问题**

```
你：帮我写一个 Python 函数，计算斐波那契数列

VelaClaw 自动选择：Claude Sonnet（擅长编程）
回答速度：中等
成本：中等
质量：优秀
```

**场景 2：日常聊天**

```
你：今天天气怎么样？

VelaClaw 自动选择：GPT-4o-mini（快速响应）
回答速度：很快
成本：低
质量：良好
```

**场景 3：复杂推理**

```
你：分析一下人工智能对未来就业市场的影响

VelaClaw 自动选择：GPT-4o（擅长推理分析）
回答速度：较慢
成本：较高
质量：优秀
```

### 如何设置偏好？

你可以告诉 VelaClaw 你的优先级：

**优先速度（适合日常聊天）**：
```yaml
routing:
  preference: speed
```

**优先质量（适合重要任务）**：
```yaml
routing:
  preference: quality
```

**优先省钱（适合大量使用）**：
```yaml
routing:
  preference: cost
```

---

## 多专家协商

### 什么是多专家协商？

想象你遇到了一个难题，你可以：
- 问一个专家，听他的答案
- 问多个专家，综合他们的意见

VelaClaw 的"多专家协商"就是后者——它会同时让多个 AI 模型回答你的问题，然后智能地综合出最佳答案。

### 为什么需要这个？

| 单一模型 | 多模型协商 |
|----------|------------|
| 可能出错 | 多个模型互相验证 |
| 单一视角 | 多角度分析 |
| 不确定性高 | 置信度更高 |

### 协商方式

VelaClaw 提供多种协商策略：

#### 1. 投票模式（默认）

**适合**：事实性问题、选择题

**工作方式**：多个模型投票，多数胜出

**例子**：
```
问题：地球到月球的距离是多少？

模型 A 说：38万公里
模型 B 说：384,400公里
模型 C 说：约38万公里

最终答案：约38万公里（3票中有2票支持）
```

#### 2. 最佳答案模式

**适合**：创意写作、开放性问题

**工作方式**：选择质量最高的回答

**例子**：
```
问题：写一首关于春天的诗

VelaClaw 让三个模型各写一首诗，
然后选择写得最好的一首给你。
```

#### 3. 级联优化模式

**适合**：复杂推理、学术论文

**工作方式**：第一个模型回答，后续模型优化

**例子**：
```
问题：设计一个电商系统架构

模型 A：给出初步设计
模型 B：发现潜在问题并改进
模型 C：进一步优化细节

最终输出：经过三轮优化的完整设计
```

### 如何启用？

**方法 1：命令行**

```bash
# 使用投票模式
velaclaw --negotiate voting

# 使用最佳答案模式
velaclaw --negotiate best

# 使用级联优化模式
velaclaw --negotiate cascade
```

**方法 2：配置文件**

```yaml
# ~/.velaclaw/config.yaml
negotiation:
  enabled: true
  strategy: voting  # voting/best/cascade
  models:           # 参与协商的模型
    - openai/gpt-4o
    - anthropic/claude-sonnet
    - deepseek/deepseek-chat
```

### 什么时候用多模型协商？

| 场景 | 建议 | 原因 |
|------|------|------|
| 简单问答 | 不需要 | 单模型足够 |
| 重要决策 | 推荐 | 多重验证更可靠 |
| 创意写作 | 推荐 | 获得最佳创意 |
| 编程问题 | 可选 | 复杂bug推荐使用 |
| 数据分析 | 推荐 | 结果更可靠 |

---

## 并行处理

### 什么是并行处理？

**传统方式**：一个一个任务依次处理

```
任务1 → 任务2 → 任务3 → 任务4
总时间 = 1+1+1+1 = 4 单位时间
```

**并行处理**：多个任务同时处理

```
任务1 ─┐
任务2 ─┼→ 同时进行
任务3 ─┤
任务4 ─┘
总时间 = 1 单位时间
```

### 什么时候会用到？

#### 场景 1：批量翻译

```
你要翻译 10 个文件：
- 传统方式：逐个翻译，耗时 10 分钟
- 并行处理：同时翻译，耗时 2 分钟
```

#### 场景 2：批量分析

```
你要分析 100 条客户反馈：
- 传统方式：逐条分析，耗时 50 分钟
- 并行处理：10 条一批，耗时 5 分钟
```

#### 场景 3：多文件处理

```
你要处理 20 个文档：
- 传统方式：逐个处理
- 并行处理：多个文档同时处理
```

### 如何使用？

在配置中设置并行数量：

```yaml
# ~/.velaclaw/config.yaml
parallel:
  max_concurrent: 5  # 最多同时处理 5 个任务
  timeout_seconds: 60  # 每个任务最长 60 秒
```

---

## 远程部署

### 什么是远程部署？

把 VelaClaw 安装到远程服务器上运行，而不是在你自己的电脑上。

### 为什么需要远程部署？

| 本地运行 | 远程部署 |
|----------|----------|
| 占用本机资源 | 不占本机资源 |
| 电脑关机就停止 | 7×24 小时运行 |
| 只能自己用 | 可以多人共享 |
| 网络不稳定影响大 | 服务器网络更稳定 |

### 如何部署？

#### 步骤 1：准备服务器

你需要一台服务器（可以是云服务器），确保：
- 操作系统：Linux（推荐 Ubuntu）
- 内存：至少 2GB
- 存储：至少 10GB

#### 步骤 2：配置部署信息

编辑配置文件：

```yaml
# ~/.velaclaw/deploy.yaml
servers:
  - id: my-server
    host: 192.168.1.100  # 服务器 IP 地址
    user: deploy         # 登录用户名
    ssh_key: ~/.ssh/my-key  # SSH 密钥路径
```

#### 步骤 3：执行部署

```bash
# 部署到服务器
velaclaw deploy my-server

# 检查部署状态
velaclaw status my-server

# 如果出问题，可以回滚
velaclaw rollback my-server
```

### 部署模式选择

| 模式 | 适合场景 | 复杂度 |
|------|----------|--------|
| **直接部署** | 简单服务器 | 低 |
| **Docker 部署** | 容器化环境 | 中 |
| **系统服务** | 生产环境 | 高 |

在配置中指定：

```yaml
deploy:
  mode: direct  # direct/docker/systemd
```

---

## 常见问题

### Q1：智能模型选择会多花钱吗？

**不会。** 智能选择的目标是"在满足需求的前提下选择最经济的模型"。比如简单问题会选择便宜的模型，帮你省钱。

### Q2：多模型协商响应会变慢吗？

**会稍微慢一点**，因为需要多个模型同时处理。但对于重要问题，多等待几秒换来更可靠的答案是值得的。

### Q3：如何知道 VelaClaw 选了哪个模型？

在设置中开启调试模式：

```yaml
debug:
  show_model_selection: true
```

然后你会在回答中看到类似信息：

```
[已选择模型：claude-sonnet，原因：擅长编程，延迟：中等，成本：中等]
```

### Q4：可以只启用部分功能吗？

**可以。** 每个功能都是独立的：

```yaml
# 只启用智能选择
routing:
  smart_selection: true
negotiation:
  enabled: false
parallel:
  enabled: false
```

### Q5：远程部署安全吗？

VelaClaw 使用 SSH 密钥认证，数据传输加密。建议：
- 使用强密码保护 SSH 密钥
- 定期更换密钥
- 限制服务器访问 IP

### Q6：如何查看使用统计？

```bash
velaclaw stats
```

会显示：

```
本月使用统计：
- 总请求数：1234
- 成功率：99.2%
- 平均延迟：1.2 秒
- 总费用：$12.34

最常用模型：
1. gpt-4o-mini (45%)
2. claude-sonnet (30%)
3. deepseek-chat (25%)
```

### Q7：费用超预算了怎么办？

设置预算上限：

```yaml
budget:
  monthly_limit: 20  # 每月最多 $20
  alert_threshold: 15  # $15 时提醒
  action: stop  # stop/warn/continue
```

当达到阈值时：
- `stop`：停止使用，直到下月
- `warn`：提醒但继续
- `continue`：继续使用（仅记录）

---

## 快速配置模板

### 日常使用（推荐）

```yaml
routing:
  smart_selection: true
  preference: balanced

negotiation:
  enabled: false

parallel:
  max_concurrent: 3

budget:
  monthly_limit: 10
  action: warn
```

### 专业用户

```yaml
routing:
  smart_selection: true
  preference: quality

negotiation:
  enabled: true
  strategy: best
  models:
    - openai/gpt-4o
    - anthropic/claude-sonnet

parallel:
  max_concurrent: 10

budget:
  monthly_limit: 50
  action: stop
```

### 省钱模式

```yaml
routing:
  smart_selection: true
  preference: cost

negotiation:
  enabled: false

parallel:
  max_concurrent: 5

budget:
  monthly_limit: 5
  action: stop
```

---

## 需要帮助？

- **文档网站**：https://velaclaw.ai/docs
- **社区论坛**：https://community.velaclaw.ai
- **问题反馈**：https://github.com/velaclaw-labs/velaclaw/issues

---

*VelaClaw - 更智能的 AI 助手*
*版本：feat/ai-protocol-integration*
*更新日期：2026-02-20*
