# 第十九章：配置参考

本章提供 VelaClaw 完整配置文件的参考。

---

## 配置文件位置

```
~/.velaclaw/config.yaml
```

---

## 完整配置示例

```yaml
# ===========================================
# VelaClaw 配置文件
# ===========================================

# -------------------------------------------
# 默认 Provider 和模型
# -------------------------------------------
provider: openrouter
model: openrouter/auto

# -------------------------------------------
# Provider 配置
# -------------------------------------------
providers:
  openai:
    api_key: ${OPENAI_API_KEY}
    base_url: https://api.openai.com/v1
    
  anthropic:
    api_key: ${ANTHROPIC_API_KEY}
    
  openrouter:
    api_key: ${OPENROUTER_API_KEY}
    
  deepseek:
    api_key: ${DEEPSEEK_API_KEY}

# -------------------------------------------
# Agent 配置
# -------------------------------------------
agent:
  # 系统提示词
  system_prompt: |
    你是一个有用的 AI 助手。
    请用简洁清晰的方式回答问题。
  
  # 温度
  temperature: 0.7
  
  # 最大输出 token
  max_tokens: 4096
  
  # 最大上下文 token
  max_context_tokens: 8000

# -------------------------------------------
# 记忆配置
# -------------------------------------------
memory:
  # 后端: sqlite / markdown / none
  backend: sqlite
  
  # 启用嵌入向量（语义检索）
  embeddings:
    enabled: true
    model: text-embedding-3-small
  
  # 缓存大小
  cache_size: 100

# -------------------------------------------
# 智能路由配置
# -------------------------------------------
routing:
  # 启用智能选择
  smart_selection: true
  
  # 偏好: speed / quality / cost / balanced
  preference: balanced
  
  # 评分权重
  weights:
    latency: 0.25
    cost: 0.25
    reliability: 0.30
    quality: 0.20
  
  # 约束条件
  constraints:
    max_latency_ms: 5000
    max_cost_per_1k: 0.05

# -------------------------------------------
# 多模型协商配置
# -------------------------------------------
negotiation:
  # 启用协商
  enabled: false
  
  # 策略: voting / best / cascade / consensus / self-consistency
  strategy: voting
  
  # 参与模型
  models:
    - provider: openai
      model: gpt-4o
    - provider: anthropic
      model: claude-sonnet
    - provider: deepseek
      model: deepseek-chat
  
  # 置信度阈值
  confidence_threshold: 0.7

# -------------------------------------------
# 渠道配置
# -------------------------------------------
channels:
  # Telegram
  telegram:
    enabled: true
    token: "YOUR_BOT_TOKEN"
    allowed_users:
      - "username"
    permissions:
      default: user
      admin_users:
        - "admin_username"
  
  # Discord
  discord:
    enabled: false
    token: "YOUR_BOT_TOKEN"
    allowed_guilds:
      - 123456789
    allowed_channels:
      - 987654321
  
  # Slack
  slack:
    enabled: false
    bot_token: "xoxb-xxx"
    app_token: "xapp-xxx"

# -------------------------------------------
# 工具配置
# -------------------------------------------
tools:
  # 全局启用
  enabled: true
  
  # 禁用的工具
  disabled: []
  
  # 文件访问
  file:
    allowed_paths:
      - /home/user/documents
    forbidden_paths:
      - /etc
      - ~/.ssh
  
  # Shell 命令
  shell:
    allowed_commands:
      - ls
      - cat
      - git
    forbidden_commands:
      - rm
      - sudo
  
  # 网络访问
  network:
    allowed_domains: []
    forbidden_domains:
      - localhost

# -------------------------------------------
# 网关配置
# -------------------------------------------
gateway:
  host: 127.0.0.1
  port: 8080
  
  # CORS 设置
  cors:
    enabled: true
    origins:
      - "http://localhost:3000"

# -------------------------------------------
# 定时任务配置
# -------------------------------------------
cron:
  # 时区
  timezone: Asia/Shanghai
  
  # 任务列表
  tasks:
    - id: morning_reminder
      schedule: "0 8 * * 1-5"
      command: "发送消息: 工作日开始"
      enabled: true

# -------------------------------------------
# 安全配置
# -------------------------------------------
security:
  # 启用配对机制
  pairing_enabled: true
  
  # 审计日志
  audit:
    enabled: true
    events:
      - auth
      - tools

# -------------------------------------------
# 预算配置
# -------------------------------------------
budget:
  # 每月限额
  monthly_limit: 50
  
  # 警告阈值
  alert_threshold: 40
  
  # 超额处理: stop / warn / continue
  action: warn

# -------------------------------------------
# 日志配置
# -------------------------------------------
logging:
  # 级别: trace / debug / info / warn / error
  level: info
  
  # 文件路径
  file: ~/.velaclaw/logs/velaclaw.log
  
  # 最大文件大小（MB）
  max_size: 10
  
  # 保留文件数
  max_files: 5
```

---

## 环境变量

可以通过环境变量覆盖配置：

| 变量 | 说明 |
|------|------|
| `OPENAI_API_KEY` | OpenAI API 密钥 |
| `ANTHROPIC_API_KEY` | Anthropic API 密钥 |
| `OPENROUTER_API_KEY` | OpenRouter API 密钥 |
| `DEEPSEEK_API_KEY` | DeepSeek API 密钥 |
| `VELACLAW_CONFIG` | 配置文件路径 |
| `RUST_LOG` | 日志级别 |

---

## 配置优先级

1. 命令行参数（最高）
2. 环境变量
3. 配置文件
4. 默认值（最低）

---

## 下一步

1. [命令参考](./18-commands.md)
2. [常见问题](./20-faq.md)

---

[← 上一章：命令参考](./18-commands.md) | [返回目录](./README.md) | [下一章：常见问题 →](./20-faq.md)
