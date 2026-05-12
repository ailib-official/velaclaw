# 第七章：Telegram 配置

本章详细介绍如何配置 VelaClaw 的 Telegram 机器人。

---

## 目录

1. [创建 Telegram 机器人](#创建-telegram-机器人)
2. [配置 VelaClaw](#配置-velaclaw)
3. [用户授权](#用户授权)
4. [使用机器人](#使用机器人)
5. [高级配置](#高级配置)

---

## 创建 Telegram 机器人

### 步骤 1：找到 BotFather

1. 打开 Telegram
2. 搜索 `@BotFather`
3. 点击进入 BotFather 对话

### 步骤 2：创建新机器人

```
你: /newbot

BotFather: Alright, a new bot. How are we going to call it? 
Please choose a name for your bot.

你: My VelaClaw Bot

BotFather: Good. Now let's choose a username for your bot. 
It must end in `bot`. Like this, for example: TetrisBot or tetris_bot.

你: my_velaclaw_bot

BotFather: Done! Congratulations on your new bot...
Use this token to access the HTTP API:
123456789:ABCdefGHIjklMNOpqrsTUVwxyz

Keep your token secure...
```

### 步骤 3：保存 Token

重要！保存好你的 Token：
```
123456789:ABCdefGHIjklMNOpqrsTUVwxyz
```

---

## 配置 VelaClaw

### 方式一：交互式配置

```bash
velaclaw channel add telegram '{"token": "YOUR_BOT_TOKEN"}'
```

### 方式二：编辑配置文件

编辑 `~/.velaclaw/config.yaml`：

```yaml
channels:
  telegram:
    enabled: true
    token: "123456789:ABCdefGHIjklMNOpqrsTUVwxyz"
```

### 验证配置

```bash
velaclaw channel doctor
```

输出：

```
Telegram:
  ✓ Bot 连接正常
  ✓ Token 有效
  Bot 用户名: @my_velaclaw_bot
```

---

## 用户授权

### 获取用户 ID

**方式一：使用 @userinfobot**

1. 在 Telegram 搜索 `@userinfobot`
2. 发送任意消息
3. 它会回复你的用户 ID

```
@userinfobot: 
Your user ID: 123456789
```

**方式二：查看日志**

```bash
# 启动 VelaClaw
velaclaw daemon

# 在 Telegram 发送消息
# 查看日志
tail -f ~/.velaclaw/logs/velaclaw.log
```

日志会显示：
```
[INFO] 收到消息来自用户: 123456789 (username)
```

### 添加用户到白名单

**方式一：命令行**

```bash
# 通过用户名添加
velaclaw channel bind-telegram your_username

# 通过用户 ID 添加
velaclaw channel bind-telegram 123456789
```

**方式二：编辑配置文件**

```yaml
channels:
  telegram:
    enabled: true
    token: "YOUR_BOT_TOKEN"
    allowed_users:
      - "your_username"
      - 123456789
      - "friend_username"
```

### 配对流程

首次使用时，VelaClaw 需要配对：

```
用户在 Telegram: 你好

VelaClaw: 
🔐 这是您第一次使用此 VelaClaw 实例。
请在您的 VelaClaw 服务器上运行以下命令完成配对：

velaclaw channel bind-telegram 123456789

或者输入配对码: 1234
```

在服务器上：

```bash
# 方式一：直接绑定
velaclaw channel bind-telegram 123456789

# 方式二：输入配对码
# 在交互模式中输入收到的配对码
```

---

## 使用机器人

### 启动服务

```bash
# 方式一：只启动渠道
velaclaw channel start

# 方式二：启动完整守护进程
velaclaw daemon
```

### 基本对话

```
你: 你好

VelaClaw: 你好！有什么我可以帮你的吗？

你: 帮我查一下天气

VelaClaw: 抱歉，我目前无法获取实时天气数据...
```

### 使用命令

在 Telegram 中使用斜杠命令：

| 命令 | 说明 |
|------|------|
| `/start` | 开始使用 |
| `/help` | 查看帮助 |
| `/clear` | 清除对话上下文 |
| `/model` | 查看当前模型 |
| `/stats` | 查看使用统计 |

```
你: /stats

VelaClaw:
📊 使用统计
今天请求数: 15
本月请求数: 234
总费用: $2.34
```

### 发送文件

```
用户: [发送文件]

VelaClaw: 收到文件 document.pdf (1.2 MB)
需要我帮你处理这个文件吗？

用户: 是的，帮我总结内容

VelaClaw: [处理文件并返回摘要]
```

---

## 高级配置

### 设置命令菜单

```bash
# 通过 BotFather 设置命令
@BotFather: /setcommands

BotFather: Choose a bot to change the list of commands.

你: @my_velaclaw_bot

BotFather: OK. Send me a list of commands for your bot...

你:
start - 开始使用
help - 查看帮助
clear - 清除对话
model - 查看当前模型
stats - 使用统计
```

### 群组配置

```yaml
channels:
  telegram:
    enabled: true
    token: "YOUR_BOT_TOKEN"
    
    # 允许的群组
    allowed_groups:
      - -1001234567890  # 群组 ID（负数）
    
    # 群组设置
    group_settings:
      # 只有特定用户能触发回复
      reply_to_mentions_only: true
      
      # 或者回复所有人
      reply_to_all: false
```

### 消息格式

```yaml
channels:
  telegram:
    # 消息格式设置
    formatting:
      # 使用 Markdown
      parse_mode: "MarkdownV2"
      
      # 或使用 HTML
      # parse_mode: "HTML"
```

### Webhook 模式（高级）

```yaml
channels:
  telegram:
    enabled: true
    token: "YOUR_BOT_TOKEN"
    
    # 使用 Webhook（需要公网地址）
    webhook:
      enabled: true
      url: "https://your-domain.com/telegram/webhook"
      port: 8443
```

---

## 故障排除

### 机器人不回复

**检查清单**：

1. 检查服务是否运行
   ```bash
   velaclaw status
   ```

2. 检查 Token 是否正确
   ```bash
   velaclaw channel doctor
   ```

3. 检查用户是否在白名单
   ```bash
   velaclaw channel list
   ```

4. 查看日志
   ```bash
   tail -f ~/.velaclaw/logs/velaclaw.log
   ```

### Token 无效

```
错误: Unauthorized

解决:
1. 检查 Token 是否正确
2. 如果 Token 泄露，通过 @BotFather 重新生成
   /revoke
```

### 权限问题

```yaml
# 确保机器人有权限
channels:
  telegram:
    # 检查 allowed_users 配置
    allowed_users:
      - "your_username"  # 确保 username 正确
```

---

## 下一步

1. **设置 Discord** → [Discord 配置](./08-discord.md)
2. **了解工具系统** → [工具系统](./10-tools.md)
3. **启用智能选择** → [智能模型选择](./05-smart-routing.md)

---

[← 上一章：通信渠道概览](./06-channels.md) | [返回目录](./README.md) | [下一章：Discord 配置 →](./08-discord.md)
