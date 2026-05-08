# 第十八章：命令参考

本章提供 ZeroSpider 所有命令的快速参考。

---

## 全局命令

### 查看帮助

```bash
zerospider --help
zerospider [command] --help
```

### 查看版本

```bash
zerospider --version
```

### 查看状态

```bash
zerospider status
```

---

## 初始化命令

### onboard - 初始化配置

```bash
# 快速配置
zerospider onboard

# 交互式配置
zerospider onboard --interactive

# 指定参数
zerospider onboard --api-key sk-xxx --provider openai/gpt-5.2
```

---

## 对话命令

### agent - 与 AI 对话

```bash
# 交互模式
zerospider agent

# 单次对话
zerospider agent --message "你好"

# 指定模型
zerospider agent --provider openai --model gpt-4o

# 设置温度
zerospider agent --temperature 0.7

# 启用智能选择
zerospider agent --smart

# 启用多模型协商
zerospider agent --negotiate voting
```

---

## Provider 命令

### providers - 列出支持的 Provider

```bash
zerospider providers
```

### models - 管理模型

```bash
# 刷新模型列表
zerospider models refresh

# 刷新特定 Provider
zerospider models refresh --provider openai

# 强制刷新
zerospider models refresh --force
```

---

## 认证命令

### auth - 管理 API 认证

```bash
# 查看认证状态
zerospider auth status

# 列出认证配置
zerospider auth list

# 粘贴 Token
zerospider auth paste-token --provider anthropic

# OAuth 登录
zerospider auth login --provider openai-codex --device-code

# 切换配置
zerospider auth use --provider openai --profile work

# 删除认证
zerospider auth logout --provider openai
```

---

## 渠道命令

### channel - 管理通信渠道

```bash
# 列出渠道
zerospider channel list

# 启动渠道
zerospider channel start

# 健康检查
zerospider channel doctor

# 添加渠道
zerospider channel add telegram '{"token": "YOUR_TOKEN"}'

# 删除渠道
zerospider channel remove telegram

# 绑定 Telegram 用户
zerospider channel bind-telegram username_or_id
```

---

## 定时任务命令

### cron - 管理定时任务

```bash
# 列出任务
zerospider cron list

# 添加任务（cron 表达式）
zerospider cron add "0 9 * * *" "提醒内容"

# 添加任务（指定时间）
zerospider cron add-at "2024-12-31T23:59:00" "新年快乐"

# 添加任务（固定间隔）
zerospider cron add-every 3600000 "每小时提醒"  # 毫秒

# 添加任务（延迟执行）
zerospider cron once "30m" "30分钟后提醒"

# 删除任务
zerospider cron remove task_id

# 更新任务
zerospider cron update task_id --expression "0 10 * * *"

# 暂停/恢复
zerospider cron pause task_id
zerospider cron resume task_id

# 立即执行
zerospider cron run task_id

# 查看执行历史
zerospider cron runs
```

---

## 服务命令

### daemon - 启动守护进程

```bash
# 启动守护进程
zerospider daemon

# 指定端口
zerospider daemon --port 8080

# 指定主机
zerospider daemon --host 0.0.0.0
```

### gateway - 启动网关

```bash
zerospider gateway --port 8080
```

### service - 系统服务管理

```bash
# 安装服务
zerospider service install

# 启动服务
zerospider service start

# 停止服务
zerospider service stop

# 查看状态
zerospider service status

# 卸载服务
zerospider service uninstall
```

---

## 诊断命令

### doctor - 运行诊断

```bash
# 全面诊断
zerospider doctor

# 检查模型
zerospider doctor models

# 检查特定 Provider
zerospider doctor models --provider openai
```

---

## 技能命令

### skills - 管理技能

```bash
# 列出技能
zerospider skills list

# 安装技能
zerospider skills install https://github.com/user/skill

# 删除技能
zerospider skills remove skill_name
```

---

## 配置命令

### config - 管理配置

```bash
# 查看配置 Schema
zerospider config schema
```

---

## 集成命令

### integrations - 查看集成

```bash
# 查看集成详情
zerospider integrations info composio
```

---

## 硬件命令

### hardware - 硬件发现

```bash
# 发现 USB 设备
zerospider hardware discover

# 检查设备
zerospider hardware introspect --path /dev/ttyACM0

# 获取芯片信息
zerospider hardware info --chip STM32F401RETx
```

### peripheral - 外设管理

```bash
# 列出外设
zerospider peripheral list

# 添加外设
zerospider peripheral add nucleo-f401re /dev/ttyACM0

# 刷写固件
zerospider peripheral flash --port /dev/ttyACM0
```

---

## 迁移命令

### migrate - 数据迁移

```bash
# 从 OpenClaw 迁移
zerospider migrate openclaw

# 指定源目录
zerospider migrate openclaw --source /path/to/openclaw

# 预览（不写入）
zerospider migrate openclaw --dry-run
```

---

## 部署命令

### deploy - 远程部署

```bash
# 部署到服务器
zerospider deploy server_name

# 查看状态
zerospider deploy status server_name

# 健康检查
zerospider deploy health-check server_name

# 更新
zerospider deploy update server_name

# 回滚
zerospider deploy rollback server_name

# 查看日志
zerospider deploy logs server_name --follow

# 重启
zerospider deploy restart server_name
```

---

## 常用选项

| 选项 | 说明 |
|------|------|
| `--help, -h` | 显示帮助 |
| `--version, -V` | 显示版本 |
| `--verbose, -v` | 详细输出 |
| `--quiet, -q` | 静默模式 |
| `--config` | 指定配置文件 |
| `--log-level` | 日志级别 |

---

## 下一步

1. [配置参考](./19-config.md)
2. [常见问题](./20-faq.md)

---

[← 上一章：安全设置](./17-security.md) | [返回目录](./README.md) | [下一章：配置参考 →](./19-config.md)
