# 第十章：工具系统

本章介绍 VelaClaw 的工具系统，了解 AI 能执行哪些操作。

---

## 目录

1. [工具概述](#工具概述)
2. [内置工具](#内置工具)
3. [工具权限](#工具权限)
4. [自定义工具](#自定义工具)

---

## 工具概述

### 什么是工具？

工具是 AI 可以调用的能力。当 AI 判断需要执行某些操作时，会自动调用相应工具。

**例子**：

```
用户: 帮我查看 config.yaml 文件的内容

AI 分析: 需要读取文件
→ 调用 file_read 工具
→ 返回文件内容
→ AI 生成回复
```

### 工作流程

```
用户请求
    │
    ▼
AI 分析请求
    │
    ├── 不需要工具 → 直接回答
    │
    └── 需要工具 → 调用工具
                      │
                      ▼
                  执行工具操作
                      │
                      ▼
                  返回结果给 AI
                      │
                      ▼
                  AI 生成回复
```

---

## 内置工具

### 文件操作

| 工具 | 功能 | 示例用法 |
|------|------|----------|
| `file_read` | 读取文件内容 | "读取 config.yaml" |
| `file_write` | 写入文件内容 | "创建 notes.txt，内容是..." |
| `image_info` | 获取图片信息 | "这张图片多大？" |

**示例**：

```
用户: 帮我读取 /home/user/notes.txt 的内容

VelaClaw: [调用 file_read 工具]
文件内容如下：
- 今天学习了 Rust
- 完成了项目配置
- 明天计划...

用户: 在文件末尾添加一行：完成了 VelaClaw 配置

VelaClaw: [调用 file_write 工具]
已添加内容到 notes.txt
```

### 网络操作

| 工具 | 功能 | 示例用法 |
|------|------|----------|
| `web_search` | 搜索网页 | "搜索 Rust 教程" |
| `http_request` | 发送 HTTP 请求 | "调用 API 获取数据" |
| `browser_open` | 打开浏览器 | "打开这个网页" |
| `screenshot` | 截图网页 | "截取网页图片" |

**示例**：

```
用户: 搜索最新的 Rust 版本

VelaClaw: [调用 web_search 工具]
搜索结果：
1. Rust 1.76.0 发布 (2024-02-08)
   主要更新：async fn 增强、性能优化...
   
2. Rust 官方网站
   https://www.rust-lang.org
   
...
```

### 系统操作

| 工具 | 功能 | 示例用法 |
|------|------|----------|
| `shell` | 执行命令 | "运行 ls -la" |
| `schedule` | 创建定时任务 | "每天早上提醒我" |

**示例**：

```
用户: 帮我查看当前目录的文件

VelaClaw: [调用 shell 工具]
当前目录文件：
drwxr-xr-x  src/
-rw-r--r--  config.yaml
-rw-r--r--  README.md
```

### 记忆操作

| 工具 | 功能 | 示例用法 |
|------|------|----------|
| `memory_store` | 存储信息 | "记住我的生日是..." |
| `memory_recall` | 检索信息 | "我的生日是什么？" |
| `memory_forget` | 删除记忆 | "忘记之前的信息" |

**示例**：

```
用户: 记住我的邮箱是 user@example.com

VelaClaw: [调用 memory_store 工具]
已记住：你的邮箱是 user@example.com

用户: 我的邮箱是什么？

VelaClaw: [调用 memory_recall 工具]
你的邮箱是 user@example.com
```

### 定时任务

| 工具 | 功能 | 示例用法 |
|------|------|----------|
| `cron_add` | 添加定时任务 | "每周一提醒我开会" |
| `cron_list` | 查看任务列表 | "列出所有定时任务" |
| `cron_remove` | 删除任务 | "取消早上 8 点的提醒" |
| `cron_run` | 立即执行任务 | "现在执行任务" |

**示例**：

```
用户: 每天 9 点提醒我开会

VelaClaw: [调用 cron_add 工具]
已创建定时任务：
- 时间：每天 9:00
- 内容：提醒开会
- 任务 ID：task_001
```

### Git 操作

| 工具 | 功能 | 示例用法 |
|------|------|----------|
| `git_operations` | Git 命令 | "帮我提交代码" |

**示例**：

```
用户: 帮我查看 git 状态

VelaClaw: [调用 git_operations 工具]
Git 状态：
On branch main
Changes not staged:
  modified: src/main.rs
  
Untracked files:
  src/new_file.rs
```

---

## 工具权限

### 启用/禁用工具

编辑 `~/.velaclaw/config.yaml`：

```yaml
tools:
  # 全局启用/禁用
  enabled: true
  
  # 禁用特定工具
  disabled:
    - shell        # 禁用命令执行
    - browser_open # 禁用浏览器
  
  # 只启用特定工具
  allowed:
    - file_read
    - file_write
    - memory_store
    - memory_recall
```

### 文件访问限制

```yaml
tools:
  file:
    # 允许访问的目录
    allowed_paths:
      - /home/user/documents
      - /home/user/projects
    
    # 禁止访问的目录
    forbidden_paths:
      - /etc
      - /root
      - ~/.ssh
```

### Shell 命令限制

```yaml
tools:
  shell:
    # 允许的命令
    allowed_commands:
      - ls
      - cat
      - echo
      - git
    
    # 禁止的命令
    forbidden_commands:
      - rm
      - sudo
      - chmod
    
    # 或完全禁用
    enabled: false
```

### 网络访问限制

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
```

---

## 自定义工具

### 添加自定义工具

VelaClaw 支持通过技能系统添加自定义工具。

**示例：天气查询工具**

1. 创建技能文件 `~/.velaclaw/skills/weather.yaml`：

```yaml
name: weather
description: 查询天气信息
tools:
  - name: get_weather
    description: 获取指定城市的天气
    parameters:
      type: object
      properties:
        city:
          type: string
          description: 城市名称
      required:
        - city
    execute:
      type: http
      url: "https://api.weather.com/v1/current?city=${city}"
      method: GET
```

2. 安装技能：

```bash
velaclaw skills install ~/.velaclaw/skills/weather.yaml
```

3. 使用：

```
用户: 北京今天天气怎么样？

VelaClaw: [调用 get_weather 工具]
北京今天天气：
- 温度：15°C
- 天气：晴
- 风力：微风
```

### 使用外部工具

VelaClaw 集成了 Composio 等外部工具平台：

```yaml
integrations:
  composio:
    enabled: true
    api_key: "YOUR_COMPOSIO_KEY"
```

启用后可访问 100+ 外部工具：
- Gmail 操作
- Slack 消息
- Google 日历
- GitHub 操作
- 更多...

---

## 工具调用日志

### 查看工具调用历史

```bash
velaclaw logs tools
```

输出：

```
最近工具调用：
2024-02-20 10:30:15 | file_read  | config.yaml
2024-02-20 10:31:22 | web_search  | "Rust tutorial"
2024-02-20 10:32:05 | shell       | ls -la
```

### 调试模式

```bash
# 启用工具调用调试
velaclaw agent --debug-tools
```

输出：

```
[DEBUG] 工具调用请求: file_read
[DEBUG] 参数: {"path": "config.yaml"}
[DEBUG] 执行结果: 成功
[DEBUG] 返回: 文件内容 (1.2 KB)
```

---

## 安全注意事项

### 不要禁用安全限制

```yaml
# 危险！不要这样做
tools:
  shell:
    allowed_commands: []  # 允许所有命令
  file:
    allowed_paths: []     # 允许访问所有文件
```

### 定期审查工具使用

```bash
# 查看工具使用统计
velaclaw stats --tools
```

### 敏感操作确认

```yaml
# 启用敏感操作确认
tools:
  require_confirmation:
    - shell
    - file_write
    - http_request
```

---

## 下一步

1. **了解技能系统** → [技能系统](./11-skills.md)
2. **配置记忆系统** → [记忆系统](./12-memory.md)
3. **设置定时任务** → [自动化与定时任务](./14-automation.md)

---

[← 上一章：其他渠道](./09-other-channels.md) | [返回目录](./README.md) | [下一章：技能系统 →](./11-skills.md)
