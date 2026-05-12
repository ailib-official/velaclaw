# 第十四章：自动化与定时任务

本章介绍如何让 VelaClaw 自动执行定时任务。

---

## 目录

1. [功能概述](#功能概述)
2. [管理定时任务](#管理定时任务)
3. [任务类型](#任务类型)
4. [配置示例](#配置示例)

---

## 功能概述

### 什么是定时任务？

定时任务让你可以：

- 每天早上自动发送提醒
- 每周生成报告
- 定期执行数据备份
- 在特定时间执行特定操作

### 工作原理

```
定时器触发
    │
    ▼
执行预设任务
    │
    ├── 发送消息到渠道
    ├── 执行命令
    └── 调用 AI 处理
```

---

## 管理定时任务

### 查看任务列表

```bash
velaclaw cron list
```

输出：

```
定时任务列表
============

┌─────────┬──────────────┬─────────────────┬────────┐
│ ID      │ 时间         │ 命令            │ 状态   │
├─────────┼──────────────┼─────────────────┼────────┤
│ task_1  │ 每天 9:00    │ remind meeting  │ ✓ 启用 │
│ task_2  │ 每周一 8:00  │ report weekly   │ ✓ 启用 │
│ task_3  │ 每月 1 日     │ backup          │ ✗ 暂停 │
└─────────┴──────────────┴─────────────────┴────────┘
```

### 添加任务

**方式一：Cron 表达式**

```bash
# 每天 9 点执行
velaclaw cron add "0 9 * * *" "提醒：开会时间"

# 每周一 8 点执行
velaclaw cron add "0 8 * * 1" "发送周报"

# 每小时执行
velaclaw cron add "0 * * * *" "检查状态"
```

**Cron 表达式格式**：

```
┌───────────── 分钟 (0-59)
│ ┌───────────── 小时 (0-23)
│ │ ┌───────────── 日期 (1-31)
│ │ │ ┌───────────── 月份 (1-12)
│ │ │ │ ┌───────────── 星期 (0-6, 0=周日)
│ │ │ │ │
* * * * *
```

**常用表达式**：

| 表达式 | 含义 |
|--------|------|
| `0 9 * * *` | 每天 9:00 |
| `0 9 * * 1` | 每周一 9:00 |
| `0 9 1 * *` | 每月 1 日 9:00 |
| `*/30 * * * *` | 每 30 分钟 |
| `0 9,17 * * *` | 每天 9:00 和 17:00 |

**方式二：固定间隔**

```bash
# 每 30 分钟执行一次
velaclaw cron add-every 1800000 "检查状态"

# 参数单位：毫秒
# 1800000ms = 30分钟
```

**方式三：指定时间**

```bash
# 在特定时间执行一次
velaclaw cron add-at "2024-12-31T23:59:00" "新年快乐！"
```

**方式四：延迟执行**

```bash
# 30 分钟后执行一次
velaclaw cron once "30m" "提醒休息"

# 支持的格式：
# 30m = 30 分钟
# 2h  = 2 小时
# 1d  = 1 天
```

### 删除任务

```bash
velaclaw cron remove task_1
```

### 暂停/恢复任务

```bash
# 暂停
velaclaw cron pause task_1

# 恢复
velaclaw cron resume task_1
```

### 更新任务

```bash
# 更新时间
velaclaw cron update task_1 --expression "0 10 * * *"

# 更新命令
velaclaw cron update task_1 --command "新的提醒内容"
```

### 立即执行

```bash
velaclaw cron run task_1
```

---

## 任务类型

### 发送提醒

```bash
# 每天早上提醒
velaclaw cron add "0 8 * * *" "发送消息到 Telegram: 早上好！新的一天开始了"
```

### AI 任务

```bash
# 每天生成日报
velaclaw cron add "0 18 * * *" "AI: 总结今天的工作进展"

# AI 会根据记忆内容生成总结
```

### 执行命令

```bash
# 定期备份数据
velaclaw cron add "0 2 * * *" "EXEC: cp -r ~/.velaclaw ~/.velaclaw_backup"
```

### 查询信息

```bash
# 定期查询天气
velaclaw cron add "0 7 * * *" "搜索: 北京今天天气"
```

---

## 配置示例

### 日常工作安排

```yaml
# ~/.velaclaw/config.yaml
cron:
  tasks:
    - id: morning_reminder
      schedule: "0 8 * * 1-5"  # 周一到周五 8:00
      command: "发送消息: 工作日开始，查看今日待办"
      channels: [telegram]
      
    - id: lunch_reminder
      schedule: "0 12 * * 1-5"
      command: "发送消息: 午餐时间"
      
    - id: daily_summary
      schedule: "0 18 * * 1-5"
      command: "AI: 总结今天的工作"
```

### 健康提醒

```yaml
cron:
  tasks:
    - id: water_reminder
      schedule: "0 */2 9-18 * * 1-5"  # 工作时间每 2 小时
      command: "发送消息: 记得喝水！"
      
    - id: eye_rest
      schedule: "0 */1 9-18 * * 1-5"  # 工作时间每小时
      command: "发送消息: 让眼睛休息一下"
```

### 定期报告

```yaml
cron:
  tasks:
    - id: weekly_report
      schedule: "0 17 * * 5"  # 周五 17:00
      command: "AI: 生成本周工作总结"
      
    - id: monthly_report
      schedule: "0 9 1 * *"  # 每月 1 日 9:00
      command: "AI: 生成上月总结和本月计划"
```

---

## 时区设置

```yaml
cron:
  timezone: "Asia/Shanghai"
  
  # 或使用 IANA 时区名称
  # timezone: "America/New_York"
  # timezone: "Europe/London"
```

或在命令中指定：

```bash
velaclaw cron add "0 9 * * *" "提醒" --tz Asia/Shanghai
```

---

## 查看执行历史

```bash
velaclaw cron runs
```

输出：

```
执行历史
========

┌─────────┬──────────────────┬────────┬─────────────────┐
│ 任务 ID │ 执行时间         │ 状态   │ 结果            │
├─────────┼──────────────────┼────────┼─────────────────┤
│ task_1  │ 2024-02-20 09:00 │ ✓ 成功 │ 消息已发送      │
│ task_2  │ 2024-02-20 08:00 │ ✓ 成功 │ 报告已生成      │
│ task_3  │ 2024-02-19 09:00 │ ✗ 失败 │ 网络连接超时    │
└─────────┴──────────────────┴────────┴─────────────────┘
```

---

## 故障排除

### 任务没有执行

**检查步骤**：

1. 确认守护进程正在运行
   ```bash
   velaclaw status
   ```

2. 检查任务状态
   ```bash
   velaclaw cron list
   ```

3. 查看日志
   ```bash
   tail -f ~/.velaclaw/logs/cron.log
   ```

### 时区问题

```bash
# 检查系统时区
timedatectl

# 在配置中明确指定时区
velaclaw cron update task_1 --tz Asia/Shanghai
```

---

## 下一步

1. **部署到服务器** → [远程部署](./15-deployment.md)
2. **了解安全设置** → [安全设置](./17-security.md)
3. **查看命令参考** → [命令参考](./18-commands.md)

---

[← 上一章：多模型协商](./13-negotiation.md) | [返回目录](./README.md) | [下一章：远程部署 →](./15-deployment.md)
