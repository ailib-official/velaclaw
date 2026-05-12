# 第四章：AI 模型与 Provider

本章介绍 VelaClaw 支持的所有 AI 模型和提供商。

---

## 目录

1. [支持的 Provider](#支持的-provider)
2. [模型选择指南](#模型选择指南)
3. [配置 Provider](#配置-provider)
4. [认证管理](#认证管理)

---

## 支持的 Provider

### 查看所有 Provider

```bash
velaclaw providers
```

输出：

```
支持的 AI 提供商 (28+)：

┌──────────────┬────────────────────┬─────────────┐
│ Provider     │ 代表模型           │ 特点        │
├──────────────┼────────────────────┼─────────────┤
│ openai       │ GPT-4o, o1         │ 稳定可靠    │
│ anthropic    │ Claude Sonnet      │ 长文本优秀  │
│ openrouter   │ 聚合 100+ 模型     │ 最灵活      │
│ deepseek     │ DeepSeek Chat      │ 国产便宜    │
│ groq         │ Mixtral, Llama     │ 极速        │
│ mistral      │ Mistral Large      │ 开源优秀    │
│ together     │ 开源模型聚合       │ 选择多      │
│ fireworks    │ 快速推理           │ 便宜        │
│ perplexity   │ 联网搜索           │ 实时信息    │
│ ...          │ ...                │ ...        │
└──────────────┴────────────────────┴─────────────┘
```

### Provider 详细介绍

#### OpenAI

| 项目 | 说明 |
|------|------|
| **模型** | gpt-4o, gpt-4o-mini, o1, o1-mini, gpt-4-turbo |
| **优点** | 生态完善、稳定性高、功能全面 |
| **缺点** | 价格较高 |
| **适合** | 需要稳定可靠服务的场景 |

**价格参考**：

| 模型 | 输入价格 | 输出价格 |
|------|----------|----------|
| gpt-4o | $5/百万 token | $15/百万 token |
| gpt-4o-mini | $0.15/百万 token | $0.6/百万 token |
| o1 | $15/百万 token | $60/百万 token |

#### Anthropic (Claude)

| 项目 | 说明 |
|------|------|
| **模型** | claude-sonnet, claude-opus, claude-haiku |
| **优点** | 长文本处理强、编程能力优秀 |
| **缺点** | 国内访问可能需要代理 |
| **适合** | 编程、长文档分析 |

**价格参考**：

| 模型 | 输入价格 | 输出价格 |
|------|----------|----------|
| claude-sonnet | $3/百万 token | $15/百万 token |
| claude-opus | $15/百万 token | $75/百万 token |
| claude-haiku | $0.25/百万 token | $1.25/百万 token |

#### OpenRouter

| 项目 | 说明 |
|------|------|
| **模型** | 聚合 OpenAI、Claude、Gemini、开源模型等 100+ |
| **优点** | 一个密钥访问所有模型、按需选择 |
| **缺点** | 依赖第三方服务 |
| **适合** | 想尝试多种模型的用户 |

**特色**：
- 自动选择最便宜的模型
- 支持模型回退
- 统一计费

#### DeepSeek

| 项目 | 说明 |
|------|------|
| **模型** | deepseek-chat, deepseek-reasoner |
| **优点** | 国内访问友好、价格极低 |
| **缺点** | 功能相对基础 |
| **适合** | 预算有限、国内用户 |

**价格参考**：

| 模型 | 输入价格 | 输出价格 |
|------|----------|----------|
| deepseek-chat | ¥1/百万 token | ¥2/百万 token |
| deepseek-reasoner | ¥4/百万 token | ¥16/百万 token |

#### Groq

| 项目 | 说明 |
|------|------|
| **模型** | llama, mixtral, gemma 等开源模型 |
| **优点** | 速度极快（毫秒级响应） |
| **缺点** | 开源模型能力有限 |
| **适合** | 需要实时响应的场景 |

#### 其他 Provider

| Provider | 特点 | 适合场景 |
|----------|------|----------|
| **Mistral** | 开源模型优秀 | 欧洲合规需求 |
| **Together** | 开源模型聚合 | 尝试新模型 |
| **Fireworks** | 快速便宜 | 大量调用 |
| **Perplexity** | 联网搜索 | 实时信息 |
| **NVIDIA NIM** | GPU 优化 | 企业部署 |
| **Grok (xAI)** | 实时信息 | X 平台集成 |
| **Gemini** | Google 生态 | 多模态需求 |
| **Cohere** | 企业级 | RAG 应用 |
| **Moonshot** | 国产 | 中文处理 |
| **Qwen** | 阿里云 | 中文处理 |
| **GLM** | 智谱 | 中文处理 |
| **Minimax** | 国产 | 多模态 |

---

## 模型选择指南

### 按任务类型选择

| 任务类型 | 推荐模型 | 原因 |
|----------|----------|------|
| 日常聊天 | gpt-4o-mini, deepseek-chat | 便宜够用 |
| 编程开发 | claude-sonnet, gpt-4o | 编程能力强 |
| 复杂推理 | o1, claude-sonnet | 推理能力优秀 |
| 长文档处理 | claude-sonnet | 200K 上下文 |
| 创意写作 | gpt-4o, claude-opus | 创造性好 |
| 实时对话 | groq 模型 | 响应极快 |
| 中文处理 | deepseek, qwen, glm | 中文优化 |
| 联网查询 | perplexity | 实时信息 |

### 按预算选择

**省钱方案**：
```yaml
provider: openrouter
model: openrouter/auto  # 自动选择最便宜的
```

**平衡方案**：
```yaml
provider: openai
model: gpt-4o-mini
```

**无预算限制**：
```yaml
provider: openai
model: gpt-4o
```

### 刷新模型列表

```bash
# 刷新所有 Provider 的模型列表
velaclaw models refresh

# 刷新特定 Provider
velaclaw models refresh --provider openai

# 强制刷新（忽略缓存）
velaclaw models refresh --force
```

---

## 配置 Provider

### 基本配置

编辑 `~/.velaclaw/config.yaml`：

```yaml
# 默认 Provider 和模型
provider: openrouter
model: openrouter/auto

# Provider 配置
providers:
  openai:
    api_key: ${OPENAI_API_KEY}
    
  anthropic:
    api_key: ${ANTHROPIC_API_KEY}
    
  openrouter:
    api_key: ${OPENROUTER_API_KEY}
    
  deepseek:
    api_key: ${DEEPSEEK_API_KEY}
```

### 自定义 API 端点

```yaml
providers:
  openai:
    api_key: sk-xxx
    base_url: https://api.openai.com/v1
    
  # 使用代理或兼容服务
  custom-openai:
    api_key: sk-xxx
    base_url: https://your-proxy.com/v1
```

### 模型参数配置

```yaml
# 默认参数
defaults:
  temperature: 0.7
  max_tokens: 4096
  
# 按模型配置
models:
  gpt-4o:
    temperature: 0.5
    max_tokens: 8192
    
  claude-sonnet:
    temperature: 0.7
    max_tokens: 4096
```

---

## 认证管理

### 查看认证状态

```bash
velaclaw auth status
```

输出：

```
认证状态
========

Provider: openrouter
  状态: ✓ 已配置
  密钥有效期: 未知
  当前配置: default

Provider: anthropic
  状态: ✗ 未配置
```

### 添加认证

**方式一：环境变量**

```bash
# 在 ~/.bashrc 或 ~/.zshrc 中
export OPENAI_API_KEY="sk-xxx"
export ANTHROPIC_API_KEY="sk-xxx"
export OPENROUTER_API_KEY="sk-or-xxx"
export DEEPSEEK_API_KEY="sk-xxx"
```

**方式二：交互式配置**

```bash
velaclaw onboard --interactive
```

**方式三：手动配置**

```bash
# 粘贴 API 密钥
velaclaw auth paste-token --provider anthropic
```

### OAuth 认证

某些 Provider 支持 OAuth：

```bash
# OpenAI Codex OAuth
velaclaw auth login --provider openai-codex --device-code

# 按提示完成 OAuth 流程
```

### 切换认证配置

```bash
# 列出所有配置
velaclaw auth list

# 切换配置
velaclaw auth use --provider openai --profile work
```

### 删除认证

```bash
velaclaw auth logout --provider openai
```

---

## 常见问题

### Q: 如何查看当前使用的模型？

```bash
velaclaw status
```

### Q: 模型调用失败怎么办？

检查步骤：

1. 确认 API 密钥有效
   ```bash
   velaclaw auth status
   ```

2. 检查网络连接
   ```bash
   velaclaw doctor
   ```

3. 查看错误日志
   ```bash
   cat ~/.velaclaw/logs/error.log
   ```

### Q: 如何测试模型是否可用？

```bash
# 测试特定 Provider
velaclaw doctor models --provider openai

# 测试所有 Provider
velaclaw doctor models
```

---

## 下一步

1. **启用智能模型选择** → [智能模型选择](./05-smart-routing.md)
2. **设置多模型协商** → [多模型协商](./13-negotiation.md)
3. **配置通信渠道** → [通信渠道概览](./06-channels.md)

---

[← 上一章：与 AI 对话](./03-chat-with-ai.md) | [返回目录](./README.md) | [下一章：智能模型选择 →](./05-smart-routing.md)
