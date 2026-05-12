# 第十七章：安全设置

本章介绍 VelaClaw 的安全设置，保护你的 AI 助手。

---

## 目录

1. [安全概览](#安全概览)
2. [用户认证](#用户认证)
3. [权限控制](#权限控制)
4. [数据安全](#数据安全)
5. [安全最佳实践](#安全最佳实践)

---

## 安全概览

### 安全层级

```
┌─────────────────────────────────────┐
│         用户认证层                   │
│  (谁能使用 VelaClaw)                 │
├─────────────────────────────────────┤
│         权限控制层                   │
│  (能做什么操作)                      │
├─────────────────────────────────────┤
│         数据安全层                   │
│  (数据如何保护)                      │
├─────────────────────────────────────┤
│         网络安全层                   │
│  (网络访问控制)                      │
└─────────────────────────────────────┘
```

### 默认安全策略

VelaClaw 采用 **安全优先** 的默认配置：

- ✓ 默认拒绝所有未知用户
- ✓ 配对机制验证身份
- ✓ 敏感操作需要确认
- ✓ API 密钥加密存储
- ✓ 日志不记录敏感信息

---

## 用户认证

### 配对机制

首次在新渠道使用时，需要完成配对：

```
用户在渠道: 你好

VelaClaw: 
🔐 首次使用需要配对
请在服务器上运行：
velaclaw channel bind-telegram 123456789

或输入配对码: 2847
```

在服务器上确认：

```bash
# 方式一：直接绑定
velaclaw channel bind-telegram 123456789

# 方式二：在交互模式输入配对码
# VelaClaw 会提示输入收到的配对码
```

### 用户白名单

只允许特定用户使用：

```yaml
# Telegram
channels:
  telegram:
    allowed_users:
      - "username1"
      - 123456789  # 用户 ID

# Discord
channels:
  discord:
    allowed_guilds:
      - 123456789  # 服务器 ID
    allowed_channels:
      - 987654321  # 频道 ID
    allowed_users:
      - 111222333  # 用户 ID
```

### 权限级别

```yaml
users:
  # 普通用户
  - id: 123456789
    level: user
    
  # 管理员（更多权限）
  - id: 987654321
    level: admin
    
  # 超级管理员（完全控制）
  - id: 111222333
    level: superadmin
```

**权限对照表**：

| 操作 | user | admin | superadmin |
|------|------|-------|------------|
| 对话 | ✓ | ✓ | ✓ |
| 使用工具 | ✓ | ✓ | ✓ |
| 添加用户 | ✗ | ✓ | ✓ |
| 修改配置 | ✗ | ✗ | ✓ |
| 系统命令 | ✗ | ✗ | ✓ |

---

## 权限控制

### 工具权限

控制 AI 可以使用哪些工具：

```yaml
tools:
  # 全局开关
  enabled: true
  
  # 禁用危险工具
  disabled:
    - shell         # 禁用命令执行
    - file_write    # 禁用文件写入
  
  # 只允许特定工具
  allowed:
    - file_read
    - web_search
    - memory_store
```

### 文件访问控制

限制 AI 可以访问的文件：

```yaml
tools:
  file:
    # 允许的目录
    allowed_paths:
      - /home/user/documents
      - /home/user/projects
    
    # 禁止的目录
    forbidden_paths:
      - /etc
      - ~/.ssh
      - ~/.config
```

### 命令限制

限制可以执行的命令：

```yaml
tools:
  shell:
    # 允许的命令白名单
    allowed_commands:
      - ls
      - cat
      - git
      - python
    
    # 禁止的命令黑名单
    forbidden_commands:
      - rm
      - sudo
      - chmod
      - chown
```

### 网络访问控制

```yaml
tools:
  network:
    # 允许访问的域名
    allowed_domains:
      - api.openai.com
      - api.anthropic.com
    
    # 禁止访问的域名
    forbidden_domains:
      - internal.company.com
      - localhost
```

---

## 数据安全

### API 密钥存储

VelaClaw 自动加密存储 API 密钥：

```bash
# 密钥存储位置
~/.velaclaw/auth/
├── profiles/
│   ├── openai.default.enc
│   └── anthropic.default.enc
└── keys/
    └── master.key  # 加密主密钥
```

**安全建议**：

```bash
# 设置适当的文件权限
chmod 700 ~/.velaclaw/auth
chmod 600 ~/.velaclaw/auth/profiles/*
```

### 日志脱敏

VelaClaw 自动脱敏敏感信息：

```
# 原始日志会被处理：
# API Key: sk-abc123xyz...
# 脱敏后：
# API Key: [REDACTED]
```

### 记忆数据保护

```yaml
memory:
  # 加密存储
  encrypt: true
  
  # 敏感信息检测
  sensitive_detection: true
  
  # 自动过滤敏感词
  filter_patterns:
    - "password"
    - "secret"
    - "token"
```

---

## 安全最佳实践

### 1. 定期轮换密钥

```bash
# 每 90 天轮换 API 密钥
# 通过各 Provider 平台重新生成
```

### 2. 最小权限原则

```yaml
# 只开放必要的权限
tools:
  allowed:
    - file_read      # 只读
    - web_search     # 仅搜索
  # 不允许文件写入和命令执行
```

### 3. 监控异常行为

```bash
# 查看工具调用日志
velaclaw logs tools

# 查看认证日志
velaclaw logs auth
```

### 4. 备份与恢复

```bash
# 定期备份配置
cp -r ~/.velaclaw ~/.velaclaw.backup

# 或设置自动备份
velaclaw cron add "0 2 * * *" "EXEC: cp -r ~/.velaclaw ~/.velaclaw.backup.$(date +%Y%m%d)"
```

### 5. 更新安全补丁

```bash
# 定期更新 VelaClaw
git pull origin main
cargo build --release
```

---

## 审计日志

### 启用审计

```yaml
audit:
  enabled: true
  
  # 记录的事件
  events:
    - auth          # 认证事件
    - tools         # 工具调用
    - config        # 配置变更
    - messages      # 消息记录（可选）
```

### 查看审计日志

```bash
velaclaw logs audit
```

输出：

```
审计日志
========

2024-02-20 10:30:15 | AUTH | 用户 123456789 登录成功
2024-02-20 10:31:22 | TOOL | file_read | config.yaml
2024-02-20 10:32:05 | TOOL | web_search | "Rust tutorial"
2024-02-20 10:35:00 | AUTH | 用户 987654321 登录失败（不在白名单）
```

---

## 安全检查清单

定期检查以下项目：

| 检查项 | 状态 |
|--------|------|
| API 密钥是否加密存储 | ☐ |
| 用户白名单是否配置 | ☐ |
| 危险工具是否禁用 | ☐ |
| 文件访问是否限制 | ☐ |
| 审计日志是否启用 | ☐ |
| 配置是否定期备份 | ☐ |
| VelaClaw 是否最新版本 | ☐ |

---

## 下一步

1. [命令参考](./18-commands.md)
2. [配置参考](./19-config.md)
3. [常见问题](./20-faq.md)

---

[← 上一章：硬件外设](./16-hardware.md) | [返回目录](./README.md) | [下一章：命令参考 →](./18-commands.md)
