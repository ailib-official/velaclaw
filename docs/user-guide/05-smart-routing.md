# 第五章：智能模型选择

本章介绍 VelaClaw 的智能模型选择功能，让 AI 自动为你选择最合适的模型。

---

## 目录

1. [功能概述](#功能概述)
2. [启用智能选择](#启用智能选择)
3. [工作原理](#工作原理)
4. [配置选项](#配置选项)
5. [使用示例](#使用示例)

---

## 功能概述

### 什么是智能模型选择？

想象你有多个 AI 模型可用：
- GPT-4o - 强大但贵
- GPT-4o-mini - 便宜够用
- Claude Sonnet - 擅长编程
- DeepSeek - 最便宜

**传统方式**：你每次都要手动选择用哪个模型

**智能选择**：VelaClaw 自动判断你的问题类型，选择最合适的模型

### 核心能力

| 能力 | 说明 |
|------|------|
| **任务识别** | 自动识别编程、翻译、聊天等任务类型 |
| **评分系统** | 根据延迟、成本、可靠性、质量评估模型 |
| **自适应选择** | 根据你的偏好选择最佳模型 |
| **实时优化** | 根据实际使用情况持续优化 |

---

## 启用智能选择

### 方式一：命令行

```bash
# 启用智能选择
velaclaw agent --smart
```

### 方式二：配置文件

编辑 `~/.velaclaw/config.yaml`：

```yaml
routing:
  smart_selection: true
```

### 方式三：启动时指定

```bash
velaclaw daemon --smart
```

---

## 工作原理

### 四维评分系统

VelaClaw 从四个维度评估每个模型：

```
评分 = 延迟分数 × 权重 
     + 成本分数 × 权重 
     + 可靠性分数 × 权重 
     + 质量分数 × 权重
```

| 维度 | 说明 | 评估方式 |
|------|------|----------|
| **延迟** | 响应速度 | 实际请求耗时 |
| **成本** | Token 价格 | 模型定价 |
| **可靠性** | 成功率 | 失败率统计 |
| **质量** | 回答质量 | 用户反馈/置信度 |

### 任务类型识别

VelaClaw 自动识别任务类型：

| 任务类型 | 触发词示例 | 推荐模型特点 |
|----------|------------|--------------|
| **编程** | "写代码"、"debug"、"函数" | 编程能力强 |
| **推理** | "分析"、"推理"、"原因" | 推理能力强 |
| **翻译** | "翻译"、"translate" | 翻译质量高 |
| **创意** | "写故事"、"创意"、"想象" | 创造性强 |
| **摘要** | "总结"、"摘要"、"概括" | 长文本处理 |
| **聊天** | 日常对话 | 响应快、便宜 |

### 示例流程

```
用户输入: "帮我写一个 Python 排序函数"
          │
          ▼
┌─────────────────────────────┐
│  1. 任务类型识别            │
│     → 编程任务              │
└─────────────────────────────┘
          │
          ▼
┌─────────────────────────────┐
│  2. 筛选擅长编程的模型      │
│     → Claude Sonnet         │
│     → GPT-4o                │
│     → DeepSeek              │
└─────────────────────────────┘
          │
          ▼
┌─────────────────────────────┐
│  3. 根据偏好评分            │
│     Claude Sonnet: 0.85     │
│     GPT-4o: 0.82            │
│     DeepSeek: 0.75          │
└─────────────────────────────┘
          │
          ▼
┌─────────────────────────────┐
│  4. 选择最高分              │
│     → Claude Sonnet         │
└─────────────────────────────┘
          │
          ▼
用户输入: "帮我写一个 Python 排序函数"
VelaClaw: [使用 Claude Sonnet 回答]
```

---

## 配置选项

### 设置偏好

编辑 `~/.velaclaw/config.yaml`：

```yaml
routing:
  smart_selection: true
  
  # 偏好设置
  preference: balanced  # speed / quality / cost / balanced
  
  # 评分权重
  weights:
    latency: 0.25    # 速度权重
    cost: 0.25       # 成本权重
    reliability: 0.30  # 可靠性权重
    quality: 0.20    # 质量权重
```

### 偏好模式

| 模式 | 说明 | 适合场景 |
|------|------|----------|
| `speed` | 优先速度 | 实时对话 |
| `quality` | 优先质量 | 重要任务 |
| `cost` | 优先省钱 | 大量使用 |
| `balanced` | 平衡（默认） | 日常使用 |

### 约束条件

设置硬性约束：

```yaml
routing:
  constraints:
    max_latency_ms: 5000      # 最大延迟 5 秒
    max_cost_per_1k: 0.01     # 每 1K token 最多 $0.01
    min_context_window: 32000  # 最小上下文窗口
    require_tools: true        # 必须支持工具调用
```

### 自定义模型配置

```yaml
routing:
  models:
    - provider: openai
      model: gpt-4o
      tasks: [coding, reasoning]
      cost_per_1k: 0.02
      
    - provider: anthropic
      model: claude-sonnet
      tasks: [coding, reasoning, creative]
      cost_per_1k: 0.018
      
    - provider: deepseek
      model: deepseek-chat
      tasks: [chat, translation, summary]
      cost_per_1k: 0.001
```

---

## 使用示例

### 示例 1：编程任务

```
用户: 帮我写一个 Python 函数，实现二分查找

VelaClaw [自动选择 Claude Sonnet]:
好的，这是一个二分查找的实现：

```python
def binary_search(arr, target):
    """
    二分查找算法
    :param arr: 已排序的数组
    :param target: 要查找的目标值
    :return: 目标值的索引，未找到返回 -1
    """
    left, right = 0, len(arr) - 1
    
    while left <= right:
        mid = (left + right) // 2
        if arr[mid] == target:
            return mid
        elif arr[mid] < target:
            left = mid + 1
        else:
            right = mid - 1
    
    return -1
```

[已选择模型: claude-sonnet | 原因: 擅长编程 | 成本: 中等]
```

### 示例 2：简单聊天

```
用户: 今天天气怎么样？

VelaClaw [自动选择 GPT-4o-mini]:
抱歉，我无法获取实时天气信息。建议你查看手机天气应用或搜索当地天气预报。

[已选择模型: gpt-4o-mini | 原因: 简单对话 | 成本: 低]
```

### 示例 3：复杂推理

```
用户: 分析人工智能对就业市场的长期影响

VelaClaw [自动选择 GPT-4o]:
人工智能对就业市场的影响是多方面的...

[详细的长篇分析]

[已选择模型: gpt-4o | 原因: 复杂推理 | 成本: 较高]
```

### 查看选择日志

```bash
# 启用调试模式
velaclaw agent --smart --debug
```

输出：

```
[DEBUG] 任务类型: coding
[DEBUG] 候选模型: claude-sonnet (0.85), gpt-4o (0.82), deepseek (0.75)
[DEBUG] 选择: claude-sonnet
[DEBUG] 原因: 编程任务, 模型擅长, 成本适中
```

---

## 性能监控

### 查看使用统计

```bash
velaclaw stats
```

输出：

```
本月使用统计
============

总请求数: 1234
成功率: 99.2%
平均延迟: 1.2 秒
总费用: $12.34

模型使用分布:
┌────────────────┬───────┬─────────┐
│ 模型           │ 次数  │ 费用    │
├────────────────┼───────┼─────────┤
│ gpt-4o-mini    │ 617   │ $3.70   │
│ claude-sonnet  │ 370   │ $5.55   │
│ deepseek-chat  │ 247   │ $3.09   │
└────────────────┴───────┴─────────┘

智能选择效果:
- 平均节省: 35% 成本
- 平均延迟: 降低 20%
```

---

## 常见问题

### Q: 智能选择会多花钱吗？

**不会**。智能选择的目标是在满足需求的前提下选择最经济的模型。实际上，它通常会帮你省钱。

### Q: 我能覆盖智能选择吗？

**可以**。你可以随时手动指定模型：

```bash
velaclaw agent --model gpt-4o
```

### Q: 如何关闭智能选择？

```yaml
routing:
  smart_selection: false
```

或使用固定模型：

```yaml
provider: openai
model: gpt-4o-mini
```

---

## 下一步

1. **了解多模型协商** → [多模型协商](./13-negotiation.md)
2. **设置通信渠道** → [通信渠道概览](./06-channels.md)
3. **配置定时任务** → [自动化与定时任务](./14-automation.md)

---

[← 上一章：AI 模型与 Provider](./04-providers.md) | [返回目录](./README.md) | [下一章：通信渠道概览 →](./06-channels.md)
