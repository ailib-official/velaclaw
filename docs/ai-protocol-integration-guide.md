# AI Protocol Integration - User Guide

This guide covers the new features introduced in the `feat/ai-protocol-integration` branch, enabling protocol-driven provider configuration, intelligent model selection, multi-model negotiation, parallel task execution, and remote deployment.

## Quick Start

### Enable Features

Add the following features to your build:

```bash
# Enable all new features
cargo build --features ai-protocol,smart-routing,multi-model,remote-deploy

# Or enable specific features
cargo build --features ai-protocol           # Protocol-driven providers only
cargo build --features smart-routing          # Provider scoring + adaptive selection
cargo build --features multi-model            # Negotiation + parallel tasks
cargo build --features remote-deploy          # Remote deployment
```

### Prerequisites

1. **ai-protocol directory**: Set `AI_PROTOCOL_DIR` environment variable pointing to the ai-protocol repository:
   ```bash
   export AI_PROTOCOL_DIR=/path/to/ai-protocol
   ```

2. **Protocol files**: Ensure provider YAML files exist in `$AI_PROTOCOL_DIR/v1/providers/`

---

## 1. Protocol-Driven Providers

### Overview

The `ProtocolBackedProvider` bridges VelaClaw's Provider trait to ai-lib-rust's AiClient, enabling configuration-driven provider setup without code changes.

### Usage

```rust
use velaclaw::providers::ProtocolBackedProvider;

// Create a protocol-backed provider
let provider = ProtocolBackedProvider::new(
    "openai",           // provider_id
    "gpt-4o",           // model_id
    Some("sk-..."),     // API key (optional, can use env vars)
)?;

// Use like any other Provider
let response = provider.chat_with_system(
    Some("You are helpful."),
    "Hello!",
    "gpt-4o",
    0.7,
).await?;
```

### Adding New Providers

No code changes required. Simply add a YAML file:

```yaml
# $AI_PROTOCOL_DIR/v1/providers/new-provider.yaml
id: new-provider
name: New AI Provider
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

## 2. Provider Scoring System

### Overview

The scoring system evaluates providers across four dimensions:
- **Latency**: Response time (lower is better)
- **Cost**: Token pricing (lower is better)
- **Reliability**: Success rate (higher is better)
- **Quality**: Output quality score (higher is better)

### Basic Usage

```rust
use velaclaw::providers::scoring::{ProviderScorer, ScoringConfig, ScoringWeights};

// Create scorer with custom weights
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

// Record successful request
scorer.record_success(
    "openai",
    std::time::Duration::from_millis(150),
    1000,    // tokens
    2,       // cost in cents
    Some(0.9), // quality score
);

// Record failure
scorer.record_failure("openai", "timeout");

// Get score
let score = scorer.score("openai"); // 0.0 - 1.0

// Rank multiple providers
let ranked = scorer.rank_providers(&["openai", "anthropic", "deepseek"]);
// Returns: [("openai", 0.85), ("anthropic", 0.82), ("deepseek", 0.78)]
```

### Scoring Formula

```
score = w_latency × latency_score 
      + w_cost × cost_score 
      + w_reliability × success_rate 
      + w_quality × avg_quality
```

---

## 3. Adaptive Model Selector

### Overview

The selector automatically chooses the best model based on:
- Task type detection (coding, reasoning, creative, etc.)
- Provider scores
- Cost and latency constraints
- Capability requirements

### Usage

```rust
use velaclaw::providers::selector::{
    AdaptiveSelector, ModelProfile, SelectionCriteria, TaskType,
};
use velaclaw::providers::scoring::ProviderScorer;

let scorer = ProviderScorer::default();
let mut selector = AdaptiveSelector::new(scorer);

// Register models
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

// Select best model for a task
let criteria = SelectionCriteria {
    task_type: Some(TaskType::Coding),
    require_tools: true,
    max_cost_per_1k: Some(0.05),
    ..Default::default()
};

let best = selector.select_best(&criteria);

// Get top N candidates
let top_3 = selector.select_top_n(&criteria, 3);
```

### Task Type Detection

```rust
// Automatic detection from prompt
let task = TaskType::from_prompt("Write a function to sort an array");
// -> TaskType::Coding

let task = TaskType::from_prompt("Analyze the pros and cons");
// -> TaskType::Reasoning

let task = TaskType::from_prompt("Translate to Chinese");
// -> TaskType::Translation
```

---

## 4. Multi-Model Negotiation

### Overview

When multiple models provide responses, the negotiation system decides the final answer using various strategies.

### Strategies

| Strategy | Description | Best For |
|----------|-------------|----------|
| `Voting` | Majority wins | Simple queries, factual answers |
| `Consensus` | All must agree | Critical decisions |
| `Arbitration` | Highest confidence decides | When models have varying expertise |
| `Cascade` | Sequential refinement | Complex reasoning |
| `BestOfN` | Best single response wins | Quality-critical tasks |
| `SelfConsistency` | Most common answer wins | Mathematical/logical problems |

### Usage

```rust
use velaclaw::agent::negotiation::{
    Negotiator, NegotiationStrategy, ModelResponse,
};

let negotiator = Negotiator::new(NegotiationStrategy::Voting)
    .with_agreement_threshold(0.7);

let responses = vec![
    ModelResponse {
        model_id: "gpt-4o".into(),
        content: "The answer is 42".into(),
        confidence: Some(0.9),
        reasoning: None,
    },
    ModelResponse {
        model_id: "claude".into(),
        content: "The answer is 42".into(),
        confidence: Some(0.85),
        reasoning: None,
    },
    ModelResponse {
        model_id: "deepseek".into(),
        content: "The answer is 24".into(),
        confidence: Some(0.7),
        reasoning: None,
    },
];

let result = negotiator.negotiate(responses);
// result.final_response: "The answer is 42"
// result.agreement_score: 0.67
// result.strategy_used: Voting
```

---

## 5. Parallel Task Execution

### Overview

Execute tasks in parallel with various patterns for optimal throughput.

### Execution Modes

| Mode | Description | Use Case |
|------|-------------|----------|
| `MapReduce` | Parallel process + aggregate | Batch processing |
| `FanOut` | Broadcast to all processors | Multi-model queries |
| `Race` | First valid response wins | Low-latency requirements |
| `Pipeline` | Sequential stages | Complex workflows |

### Usage

```rust
use velaclaw::agent::parallel::{
    ParallelExecutor, Task, TaskProcessor,
};
use std::sync::Arc;

// Define a processor
struct Doubler;
impl TaskProcessor<i32, i32> for Doubler {
    async fn process(&self, input: i32) -> anyhow::Result<i32> {
        Ok(input * 2)
    }
}

let executor = ParallelExecutor::new(10, 30000); // concurrency, timeout_ms
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

// Race - first to complete wins
let result = executor.race(
    21,
    vec![processor.clone(), processor.clone()],
).await?;
// result = 42
```

---

## 6. Remote Deployment

### Overview

Deploy VelaClaw to remote servers with configuration-driven setup and health monitoring.

### Deployment Modes

| Mode | Description | Use Case |
|------|-------------|----------|
| `Direct` | SSH + binary upload | Simple servers |
| `Docker` | Container deployment | Containerized environments |
| `Systemd` | System service | Production Linux servers |

### Usage

```rust
use velaclaw::deploy::{
    RemoteDeployer, DeploymentTarget, DeploymentConfig, DeploymentMode,
};

let mut deployer = RemoteDeployer::new(DeploymentMode::Direct);

// Register deployment target
deployer.register_target(
    DeploymentTarget::new("prod-1", "192.168.1.100", "deploy")
        .with_port(22)
        .with_ssh_key("~/.ssh/deploy_key")
        .with_label("env", "production")
);

// Configure deployment
deployer.set_config(DeploymentConfig {
    name: "velaclaw".into(),
    version: "latest".into(),
    binary_path: "/usr/local/bin/velaclaw".into(),
    working_dir: "/opt/velaclaw".into(),
    auto_start: true,
    restart_on_failure: true,
    ..Default::default()
});

// Deploy
deployer.deploy("prod-1", "velaclaw").await?;

// Health check
let healthy = deployer.health_check("prod-1").await?;

// Rollback if needed
deployer.rollback("prod-1").await?;
```

---

## Configuration Reference

### Environment Variables

| Variable | Description |
|----------|-------------|
| `AI_PROTOCOL_DIR` | Path to ai-protocol repository |
| `AI_PROTOCOL_PATH` | Alternative path to ai-protocol |
| `AI_LIB_BATCH_CONCURRENCY` | Override batch concurrency |

### Feature Flags

| Flag | Dependencies | Description |
|------|--------------|-------------|
| `ai-protocol` | ai-lib-rust | Protocol-driven providers |
| `smart-routing` | - | Scoring + adaptive selection |
| `multi-model` | - | Negotiation + parallel tasks |
| `remote-deploy` | - | Remote deployment |

---

## Examples

### Full Integration Example

```rust
use velaclaw::providers::{
    ProtocolBackedProvider, ProviderScorer, AdaptiveSelector, SelectionCriteria, TaskType,
};
use velaclaw::agent::negotiation::{Negotiator, NegotiationStrategy};
use velaclaw::agent::parallel::ParallelExecutor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Create scorer and selector
    let scorer = ProviderScorer::default();
    let selector = AdaptiveSelector::new(scorer);

    // 2. Select best model for coding task
    let criteria = SelectionCriteria {
        task_type: Some(TaskType::Coding),
        require_tools: true,
        ..Default::default()
    };
    let best_model = selector.select_best(&criteria).unwrap();

    // 3. Create protocol-backed provider
    let provider = ProtocolBackedProvider::new(
        &best_model.provider_id,
        &best_model.model_id,
        None,
    )?;

    // 4. Execute query
    let response = provider.chat_with_system(
        Some("You are a coding assistant."),
        "Write a bubble sort in Rust",
        &best_model.model_id,
        0.7,
    ).await?;

    println!("{}", response);
    Ok(())
}
```

---

## Troubleshooting

### Protocol Not Found

```
Error: Failed to build client for openai/gpt-4o: Protocol not found
```

**Solution**: Ensure `AI_PROTOCOL_DIR` is set and provider YAML exists.

### Compilation Errors

If you see feature-related compilation errors, ensure you're building with the correct features:

```bash
cargo build --features ai-protocol,smart-routing
```

### Timeout in Parallel Execution

Increase the timeout:

```rust
let executor = ParallelExecutor::new(10, 60000); // 60 second timeout
```

---

## Migration from Legacy Providers

The protocol-backed provider is a drop-in replacement:

```rust
// Before
let provider = create_provider("openai", Some("sk-..."))?;

// After (with ai-protocol feature)
let provider = ProtocolBackedProvider::new("openai", "gpt-4o", Some("sk-..."))?;
```

Both implement the same `Provider` trait, so existing code works unchanged.
