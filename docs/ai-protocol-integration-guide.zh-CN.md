# AI 协议集成 - 用户指南

本指南涵盖 `feat/ai-protocol-integration` 分支引入的新功能，包括协议驱动的 Provider 配置、智能模型选择、多模型协商、并行任务执行和远程部署。

## 快速开始

### 启用功能

在构建时添加以下功能标志：

```bash
# 启用所有新功能
cargo build --features ai-protocol,smart-routing,multi-model,remote-deploy

# 或启用特定功能
cargo build --features ai-protocol           # 仅协议驱动 Provider
cargo build --features smart-routing          # Provider 评分 + 自适应选择
cargo build --features multi-model            # 协商 + 并行任务
cargo build --features remote-deploy          # 远程部署
```

### 前置条件

1. **ai-protocol 目录**：设置 `AI_PROTOCOL_DIR` 环境变量指向 ai-protocol 仓库：
   ```bash
   export AI_PROTOCOL_DIR=/path/to/ai-protocol
   ```

2. **协议文件**：确保 Provider YAML 文件存在于 `$AI_PROTOCOL_DIR/v1/providers/`

---

## 1. 协议驱动 Provider

### 概述

`ProtocolBackedProvider` 将 VelaClaw 的 Provider trait 桥接到 ai-lib-rust 的 AiClient，实现配置驱动的 Provider 设置，无需修改代码。

### 使用方法

```rust
use velaclaw::providers::ProtocolBackedProvider;

// 创建协议驱动的 Provider
let provider = ProtocolBackedProvider::new(
    "openai",           // provider_id
    "gpt-4o",           // model_id
    Some("sk-..."),     // API 密钥（可选，可使用环境变量）
)?;

// 像其他 Provider 一样使用
let response = provider.chat_with_system(
    Some("你是一个有用的助手。"),
    "你好！",
    "gpt-4o",
    0.7,
).await?;
```

### 添加新 Provider

无需修改代码。只需添加 YAML 文件：

```yaml
# $AI_PROTOCOL_DIR/v1/providers/new-provider.yaml
id: new-provider
name: 新 AI 提供商
base_url: https://api.newprovider.com/v1
api_style: OpenAiCompatible

auth:
  type: bearer
  header: Authorization
  prefix: "Bearer "
  env_var: NEWPROVIDER_API_KEY

streaming:
  decoder:
    type: sse
  event_map:
    - match_expr: "$.choices[0].delta.content != null"
      emit: "PartialContentDelta"
      fields:
        content: "$.choices[0].delta.content"

capabilities:
  - text
  - streaming
  - tools
```

---

## 2. Provider 评分系统

### 概述

评分系统从四个维度评估 Provider：
- **延迟**：响应时间（越低越好）
- **成本**：Token 价格（越低越好）
- **可靠性**：成功率（越高越好）
- **质量**：输出质量评分（越高越好）

### 基本用法

```rust
use velaclaw::providers::scoring::{ProviderScorer, ScoringConfig, ScoringWeights};

// 使用自定义权重创建评分器
let config = ScoringConfig {
    weights: ScoringWeights {
        latency: 0.3,
        cost: 0.2,
        reliability: 0.3,
        quality: 0.2,
    },
    ..Default::default()
};
let mut scorer = ProviderScorer::new(config);

// 记录成功请求
scorer.record_success(
    "openai",
    std::time::Duration::from_millis(150),
    1000,      // Token 数量
    2,         // 成本（美分）
    Some(0.9), // 质量评分
);

// 记录失败
scorer.record_failure("openai", "timeout");

// 获取评分
let score = scorer.score("openai"); // 0.0 - 1.0

// 排名多个 Provider
let ranked = scorer.rank_providers(&["openai", "anthropic", "deepseek"]);
// 返回: [("openai", 0.85), ("anthropic", 0.82), ("deepseek", 0.78)]
```

### 评分公式

```
score = w_latency × 延迟分数 
      + w_cost × 成本分数 
      + w_reliability × 成功率 
      + w_quality × 平均质量
```

---

## 3. 自适应模型选择器

### 概述

选择器根据以下因素自动选择最佳模型：
- 任务类型检测（编码、推理、创意等）
- Provider 评分
- 成本和延迟约束
- 能力要求

### 使用方法

```rust
use velaclaw::providers::selector::{
    AdaptiveSelector, ModelProfile, SelectionCriteria, TaskType,
};
use velaclaw::providers::scoring::ProviderScorer;

let scorer = ProviderScorer::default();
let mut selector = AdaptiveSelector::new(scorer);

// 注册模型
selector.register_model(ModelProfile {
    provider_id: "openai".into(),
    model_id: "gpt-4o".into(),
    supported_tasks: vec![TaskType::Coding, TaskType::Reasoning],
    context_window: 128000,
    supports_vision: true,
    supports_tools: true,
    supports_streaming: true,
    input_cost_per_1k: 0.005,
    output_cost_per_1k: 0.015,
    avg_latency_ms: 800,
});

// 为任务选择最佳模型
let criteria = SelectionCriteria {
    task_type: Some(TaskType::Coding),
    require_tools: true,
    max_cost_per_1k: Some(0.05),
    ..Default::default()
};

let best = selector.select_best(&criteria);

// 获取前 N 个候选
let top_3 = selector.select_top_n(&criteria, 3);
```

### 任务类型检测

```rust
// 从提示词自动检测
let task = TaskType::from_prompt("写一个排序数组的函数");
// -> TaskType::Coding

let task = TaskType::from_prompt("分析优缺点");
// -> TaskType::Reasoning

let task = TaskType::from_prompt("翻译成中文");
// -> TaskType::Translation
```

---

## 4. 多模型协商

### 概述

当多个模型提供响应时，协商系统使用各种策略决定最终答案。

### 策略

| 策略 | 描述 | 适用场景 |
|------|------|----------|
| `Voting` | 多数胜出 | 简单查询、事实性答案 |
| `Consensus` | 全部同意 | 关键决策 |
| `Arbitration` | 最高置信度决定 | 模型有不同专长 |
| `Cascade` | 顺序优化 | 复杂推理 |
| `BestOfN` | 最佳单响应胜出 | 质量关键任务 |
| `SelfConsistency` | 最常见答案胜出 | 数学/逻辑问题 |

### 使用方法

```rust
use velaclaw::agent::negotiation::{
    Negotiator, NegotiationStrategy, ModelResponse,
};

let negotiator = Negotiator::new(NegotiationStrategy::Voting)
    .with_agreement_threshold(0.7);

let responses = vec![
    ModelResponse {
        model_id: "gpt-4o".into(),
        content: "答案是 42".into(),
        confidence: Some(0.9),
        reasoning: None,
    },
    ModelResponse {
        model_id: "claude".into(),
        content: "答案是 42".into(),
        confidence: Some(0.85),
        reasoning: None,
    },
    ModelResponse {
        model_id: "deepseek".into(),
        content: "答案是 24".into(),
        confidence: Some(0.7),
        reasoning: None,
    },
];

let result = negotiator.negotiate(responses);
// result.final_response: "答案是 42"
// result.agreement_score: 0.67
// result.strategy_used: Voting
```

---

## 5. 并行任务执行

### 概述

使用各种模式并行执行任务以获得最佳吞吐量。

### 执行模式

| 模式 | 描述 | 用例 |
|------|------|------|
| `MapReduce` | 并行处理 + 聚合 | 批量处理 |
| `FanOut` | 广播到所有处理器 | 多模型查询 |
| `Race` | 第一个有效响应胜出 | 低延迟需求 |
| `Pipeline` | 顺序阶段 | 复杂工作流 |

### 使用方法

```rust
use velaclaw::agent::parallel::{
    ParallelExecutor, Task, TaskProcessor,
};
use std::sync::Arc;

// 定义处理器
struct Doubler;
impl TaskProcessor<i32, i32> for Doubler {
    async fn process(&self, input: i32) -> anyhow::Result<i32> {
        Ok(input * 2)
    }
}

let executor = ParallelExecutor::new(10, 30000); // 并发数, 超时毫秒
let processor = Arc::new(Doubler);

// Map-Reduce
let tasks = vec![
    Task { id: "1".into(), input: 1 },
    Task { id: "2".into(), input: 2 },
    Task { id: "3".into(), input: 3 },
];

let result = executor.map_reduce(
    tasks,
    processor.clone(),
    |results| results.into_iter().filter_map(|r| r.ok()).sum()
).await?;
// result = 12

// Race - 第一个完成者胜出
let result = executor.race(
    21,
    vec![processor.clone(), processor.clone()],
).await?;
// result = 42
```

---

## 6. 远程部署

### 概述

使用配置驱动的设置和健康监控将 VelaClaw 部署到远程服务器。

### 部署模式

| 模式 | 描述 | 用例 |
|------|------|------|
| `Direct` | SSH + 二进制上传 | 简单服务器 |
| `Docker` | 容器部署 | 容器化环境 |
| `Systemd` | 系统服务 | 生产 Linux 服务器 |

### 使用方法

```rust
use velaclaw::deploy::{
    RemoteDeployer, DeploymentTarget, DeploymentConfig, DeploymentMode,
};

let mut deployer = RemoteDeployer::new(DeploymentMode::Direct);

// 注册部署目标
deployer.register_target(
    DeploymentTarget::new("prod-1", "192.168.1.100", "deploy")
        .with_port(22)
        .with_ssh_key("~/.ssh/deploy_key")
        .with_label("env", "production")
);

// 配置部署
deployer.set_config(DeploymentConfig {
    name: "velaclaw".into(),
    version: "latest".into(),
    binary_path: "/usr/local/bin/velaclaw".into(),
    working_dir: "/opt/velaclaw".into(),
    auto_start: true,
    restart_on_failure: true,
    ..Default::default()
});

// 部署
deployer.deploy("prod-1", "velaclaw").await?;

// 健康检查
let healthy = deployer.health_check("prod-1").await?;

// 需要时回滚
deployer.rollback("prod-1").await?;
```

---

## 配置参考

### 环境变量

| 变量 | 描述 |
|------|------|
| `AI_PROTOCOL_DIR` | ai-protocol 仓库路径 |
| `AI_PROTOCOL_PATH` | ai-protocol 备用路径 |
| `AI_LIB_BATCH_CONCURRENCY` | 覆盖批量并发数 |

### 功能标志

| 标志 | 依赖 | 描述 |
|------|------|------|
| `ai-protocol` | ai-lib-rust | 协议驱动 Provider |
| `smart-routing` | - | 评分 + 自适应选择 |
| `multi-model` | - | 协商 + 并行任务 |
| `remote-deploy` | - | 远程部署 |

---

## 示例

### 完整集成示例

```rust
use velaclaw::providers::{
    ProtocolBackedProvider, ProviderScorer, AdaptiveSelector, SelectionCriteria, TaskType,
};
use velaclaw::agent::negotiation::{Negotiator, NegotiationStrategy};
use velaclaw::agent::parallel::ParallelExecutor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 创建评分器和选择器
    let scorer = ProviderScorer::default();
    let selector = AdaptiveSelector::new(scorer);

    // 2. 为编码任务选择最佳模型
    let criteria = SelectionCriteria {
        task_type: Some(TaskType::Coding),
        require_tools: true,
        ..Default::default()
    };
    let best_model = selector.select_best(&criteria).unwrap();

    // 3. 创建协议驱动 Provider
    let provider = ProtocolBackedProvider::new(
        &best_model.provider_id,
        &best_model.model_id,
        None,
    )?;

    // 4. 执行查询
    let response = provider.chat_with_system(
        Some("你是一个编码助手。"),
        "用 Rust 写一个冒泡排序",
        &best_model.model_id,
        0.7,
    ).await?;

    println!("{}", response);
    Ok(())
}
```

---

## 故障排除

### 协议未找到

```
Error: Failed to build client for openai/gpt-4o: Protocol not found
```

**解决方案**：确保设置了 `AI_PROTOCOL_DIR` 并且 Provider YAML 文件存在。

### 编译错误

如果看到与功能相关的编译错误，确保使用正确的功能标志构建：

```bash
cargo build --features ai-protocol,smart-routing
```

### 并行执行超时

增加超时时间：

```rust
let executor = ParallelExecutor::new(10, 60000); // 60 秒超时
```

---

## 从旧版 Provider 迁移

协议驱动 Provider 是直接替换：

```rust
// 之前
let provider = create_provider("openai", Some("sk-..."))?;

// 之后（使用 ai-protocol 功能）
let provider = ProtocolBackedProvider::new("openai", "gpt-4o", Some("sk-..."))?;
```

两者实现相同的 `Provider` trait，现有代码无需修改即可工作。
