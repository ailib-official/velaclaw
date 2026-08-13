# 第三章：与 AI 对话

本章介绍如何与 VelaClaw 中的 AI 进行有效对话。

---

## 目录

1. [基本对话](#基本对话)
2. [对话模式](#对话模式)
3. [对话技巧](#对话技巧)
4. [高级功能](#高级功能)

---

## 基本对话

### 启动对话

**交互模式**（持续对话）：

```bash
velaclaw agent
```

```
VelaClaw 交互模式
输入消息开始对话，输入 /help 查看帮助，输入 /quit 退出。任务进行中可连按两次 Esc 停止当前 turn。。任务进行中可连按两次 Esc 停止当前 turn。

你: 你好！
VelaClaw: 你好！有什么我可以帮你的吗？

你: 帮我解释一下什么是递归
VelaClaw: 递归是一种编程技术...

你: /quit
再见！
```

**单次对话**（一问一答）：

```bash
velaclaw agent --message "什么是机器学习？"
```

### 终端输出渲染

交互模式下，TTY 会话会把常见 Markdown 渲染为终端友好格式，并对超长工具输出折叠；可用 `/expand <id>` 展开。

```bash
velaclaw agent --no-color   # 强制纯文本
velaclaw agent --no-fold    # 禁用折叠
```

详见 [cli-render.md](../cli-render.md) 与配置节 `[cli_render]`。

### 指定模型

```bash
# 使用特定模型
velaclaw agent --model gpt-4o

# 使用特定 Provider
velaclaw agent --provider anthropic --model claude-sonnet

# 设置温度（创造性程度）
velaclaw agent --temperature 0.5
```

**温度参数说明**：

| 温度值 | 效果 | 适用场景 |
|--------|------|----------|
| 0.0 - 0.3 | 保守、确定 | 编程、事实查询 |
| 0.4 - 0.7 | 平衡 | 日常对话 |
| 0.8 - 1.0 | 创造性 | 创意写作、头脑风暴 |

---

## 对话模式

### 普通对话

最基础的对话模式，AI 根据你的问题给出回答。

```
你: 什么是光合作用？
VelaClaw: 光合作用是植物利用阳光...
```

### 带任务的对话

让 AI 帮你完成具体任务：

```
你: 帮我写一个 Python 函数，计算列表的平均值

VelaClaw: 好的，这是一个计算列表平均值的函数：

```python
def calculate_average(numbers):
    """计算列表的平均值"""
    if not numbers:
        return 0
    return sum(numbers) / len(numbers)

# 使用示例
numbers = [1, 2, 3, 4, 5]
print(calculate_average(numbers))  # 输出: 3.0
```

### 多轮对话

VelaClaw 会记住上下文，可以连续讨论：

```
你: 帮我写一个排序函数

VelaClaw: 好的，这是一个冒泡排序函数：
[代码...]

你: 能改成快速排序吗？

VelaClaw: 当然，这是快速排序版本：
[代码...] (AI 知道你在说之前那个排序函数)
```

### 工具调用对话

AI 会自动判断何时使用工具：

```
你: 帮我查看当前目录有什么文件

VelaClaw: [调用 file_read 工具]
当前目录有以下文件：
- config.yaml
- README.md
- src/
- tests/

你: 读取 README.md 的内容

VelaClaw: [调用 file_read 工具]
README.md 的内容是：
# 项目说明
...
```

---

## 对话技巧

### 写好提示词

**不好的提示**：
```
写代码
```

**好的提示**：
```
写一个 Python 函数，接收一个整数列表，返回排序后的列表。
要求：
1. 使用快速排序算法
2. 添加类型注解
3. 包含示例用法
```

### 结构化提问

使用结构化的方式提问，可以获得更好的回答：

```
你: 我需要完成以下任务：
1. 创建一个 HTTP 服务器
2. 支持 GET 和 POST 请求
3. 返回 JSON 格式的响应

请给出完整的代码实现。
```

### 分步处理复杂任务

对于复杂任务，分步进行：

```
你: 我想开发一个待办事项应用，我们先讨论一下架构设计

VelaClaw: 好的，我们来讨论架构...

你: 数据存储用 SQLite 还是 JSON 文件更好？

VelaClaw: 我建议使用 SQLite，因为...

你: 那我们先设计数据库表结构

VelaClaw: 建议的表结构如下...
```

### 利用记忆

VelaClaw 会记住之前的对话：

```
第一天：
你: 我叫小明，是一名 Python 开发者
VelaClaw: 很高兴认识你，小明！

第二天：
你: 你还记得我的名字吗？
VelaClaw: 记得，你叫小明，是一名 Python 开发者。
```

---

## 高级功能

### 查看对话历史

```bash
# 查看最近对话
velaclaw memory recall "对话历史"
```

### 清除记忆

```
你: /forget

VelaClaw: 已清除本次对话的短期记忆。
```

### 使用系统提示

在配置文件中设置系统提示：

```yaml
# ~/.velaclaw/config.yaml
agent:
  system_prompt: |
    你是一个专业的编程助手。
    - 回答要简洁明了
    - 代码要有注释
    - 优先使用 Python
```

### 流式输出

默认情况下，VelaClaw 会流式输出 AI 的回复：

```
你: 写一个长故事

VelaClaw: 从前有一个小村庄，村里住着...
(文字逐步显示，不需要等待完整回复)
```

### 限制回复长度

```yaml
# 配置文件
agent:
  max_tokens: 2000  # 最大输出 token 数
```

---

## 命令行快捷键

在交互模式中：

| 快捷键 | 功能 |
|--------|------|
| ↑ / ↓ | 浏览历史消息 |
| Ctrl+C | 取消当前输入 |
| Ctrl+D | 退出程序 |
| Tab | 自动补全 |

---

## 常见问题

### Q: AI 回答不符合预期怎么办？

**尝试**：
1. 让 AI 解释它的理解
2. 提供更多上下文
3. 分解问题为小步骤
4. 给出例子

```
你: 这个回答不太对，我想要的是...

VelaClaw: 抱歉理解有误，让我重新理解一下...

你: 我给你举个例子：
输入: [1, 2, 3]
期望输出: 6 (1+2+3)
```

### Q: 对话上下文太长怎么办？

```
你: /clear

VelaClaw: 已清除当前对话上下文，但保留了长期记忆。
```

### Q: 如何让 AI 更专业？

在系统提示中定义角色：

```yaml
agent:
  system_prompt: |
    你是一位专业的 {{role}}。
    请用专业但易懂的方式回答问题。
```

---

## 下一步

学会了基本对话后，可以：

1. **了解支持的模型** → [AI 模型与 Provider](./04-providers.md)
2. **启用智能选择** → [智能模型选择](./05-smart-routing.md)
3. **接入更多平台** → [通信渠道概览](./06-channels.md)

---

[← 上一章：基础概念](./02-basic-concepts.md) | [返回目录](./README.md) | [下一章：AI 模型与 Provider →](./04-providers.md)
