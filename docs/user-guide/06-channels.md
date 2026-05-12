# 第六章：通信渠道概览

本章介绍 VelaClaw 支持的各种通信渠道。

---

## 目录

1. [支持的平台](#支持的平台)
2. [渠道管理](#渠道管理)
3. [渠道配置](#渠道配置)
4. [选择建议](#选择建议)

---

## 支持的平台

VelaClaw 支持以下通信渠道：

### 即时通讯平台

| 平台 | 成熟度 | 特点 |
|------|--------|------|
| **Telegram** | ⭐⭐⭐⭐⭐ | 最成熟，功能完整，推荐首选 |
| **Discord** | ⭐⭐⭐⭐ | 游戏社区友好，支持语音 |
| **Slack** | ⭐⭐⭐⭐ | 团队协作，企业友好 |
| **Matrix** | ⭐⭐⭐⭐ | 开源协议，隐私优先 |
| **Signal** | ⭐⭐⭐ | 隐私保护最强 |
| **IRC** | ⭐⭐⭐ | 传统协议，轻量 |

### 国内平台

| 平台 | 成熟度 | 特点 |
|------|--------|------|
| **微信** | ⭐⭐⭐ | 通过企业微信接入 |
| **钉钉** | ⭐⭐⭐⭐ | 企业办公友好 |
| **飞书/Lark** | ⭐⭐⭐⭐ | 字节跳动生态 |
| **QQ** | ⭐⭐⭐ | 年轻用户群体 |

### 其他渠道

| 渠道 | 特点 |
|------|------|
| **Email** | SMTP/IMAP，传统邮件 |
| **WhatsApp** | 全球用户多 |
| **iMessage** | 苹果生态 |
| **Mattermost** | 开源团队协作 |
| **Web Gateway** | 自定义界面 |

---

## 渠道管理

### 查看已配置渠道

```bash
velaclaw channel list
```

输出：

```
已配置的渠道
============

┌──────────┬─────────┬────────┬──────────┐
│ 名称     │ 类型    │ 状态   │ 最后活跃 │
├──────────┼─────────┼────────┼──────────┤
│ telegram │ Telegram │ ✓ 运行 │ 2分钟前  │
│ discord  │ Discord  │ ✓ 运行 │ 5分钟前  │
│ slack    │ Slack    │ ✗ 停止 │ 1天前    │
└──────────┴─────────┴────────┴──────────┘
```

### 启动渠道

```bash
# 启动所有配置的渠道
velaclaw channel start

# 启动后，VelaClaw 会在各平台监听消息
```

### 添加渠道

```bash
# 添加 Telegram 渠道
velaclaw channel add telegram '{"token": "YOUR_BOT_TOKEN"}'

# 添加 Discord 渠道
velaclaw channel add discord '{"token": "YOUR_BOT_TOKEN"}'
```

### 删除渠道

```bash
velaclaw channel remove telegram
```

### 健康检查

```bash
velaclaw channel doctor
```

输出：

```
渠道健康检查
============

Telegram:
  ✓ Bot 连接正常
  ✓ Webhook 配置正确
  ✓ 有权发送消息

Discord:
  ✓ Bot 连接正常
  ⚠ 缺少发送消息权限
  → 建议: 检查 Bot 权限设置
```

---

## 渠道配置

### 配置文件结构

编辑 `~/.velaclaw/config.yaml`：

```yaml
channels:
  # Telegram 配置
  telegram:
    enabled: true
    token: "YOUR_BOT_TOKEN"
    allowed_users:
      - "your_username"
      - 123456789  # 用户 ID
    
  # Discord 配置
  discord:
    enabled: true
    token: "YOUR_BOT_TOKEN"
    allowed_guilds:
      - 123456789  # 服务器 ID
    allowed_channels:
      - 987654321  # 频道 ID
      
  # Slack 配置
  slack:
    enabled: false
    bot_token: "xoxb-xxx"
    app_token: "xapp-xxx"
```

### 用户白名单

限制谁可以使用你的 VelaClaw：

```yaml
# Telegram 用户白名单
channels:
  telegram:
    allowed_users:
      - "username1"
      - 123456789  # 数字用户 ID
```

```yaml
# Discord 服务器白名单
channels:
  discord:
    allowed_guilds:
      - 123456789
```

### 权限配置

```yaml
channels:
  telegram:
    # 权限级别
    permissions:
      default: user      # 默认权限
      admin_users:
        - "your_username"
      # 管理员可以执行更多操作
```

---

## 选择建议

### 个人使用

| 推荐平台 | 原因 |
|----------|------|
| **Telegram** | 功能完整，配置简单，免费 |
| **Discord** | 如果你已经在使用 Discord |

### 团队使用

| 推荐平台 | 原因 |
|----------|------|
| **Slack** | 企业集成好，权限管理完善 |
| **Discord** | 小团队，免费 |
| **飞书/钉钉** | 国内企业 |

### 隐私优先

| 推荐平台 | 原因 |
|----------|------|
| **Signal** | 端到端加密 |
| **Matrix** | 开源，自建服务器 |

### 国内用户

| 推荐平台 | 原因 |
|----------|------|
| **钉钉/飞书** | 国内访问稳定 |
| **Telegram** | 需要稳定网络 |

---

## 下一步

1. **设置 Telegram** → [Telegram 配置](./07-telegram.md)
2. **设置 Discord** → [Discord 配置](./08-discord.md)
3. **了解工具系统** → [工具系统](./10-tools.md)

---

[← 上一章：智能模型选择](./05-smart-routing.md) | [返回目录](./README.md) | [下一章：Telegram 配置 →](./07-telegram.md)
