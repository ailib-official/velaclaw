# 上游项目需求总结

## 概述

在将 ai-lib-rust 集成到 VelaClaw 的过程中，发现了一些差距和改进机会。本文档总结了 ai-lib-rust 和 ai-protocol 项目的新需求。

---

## 1. ai-lib-rust 需求

### 1.1 API 接口改进

#### 问题：ChatRequestBuilder 缺少 `model()` 方法

**当前状态**：
```rust
// ChatRequestBuilder 没有 model() 方法
let response = client.chat()
    .messages(messages)
    .temperature(0.7)
    .execute()  // 模型必须在创建客户端时设置
    .await?;
```

**需求**：为 `ChatRequestBuilder` 添加 `model()` 方法，支持按请求覆盖模型：

```rust
let response = client.chat()
    .messages(messages)
    .model("gpt-4o")  // 新增：按请求覆盖模型
    .temperature(0.7)
    .execute()
    .await?;
```

**收益**：实现单客户端多模型使用模式。

---

#### 问题：AiClient 不可克隆

**当前状态**：
```rust
pub struct AiClient { ... }  // 没有 Clone 实现
```

**需求**：为 `AiClient` 实现 `Clone`：

```rust
#[derive(Clone)]
pub struct AiClient { ... }
```

**用例**：在异步任务、流式上下文中共享客户端。

---

#### 问题：ToolDefinition 与通用 JSON 工具

**当前状态**：
```rust
pub fn tools(mut self, tools: Vec<ToolDefinition>) -> Self;
```

**需求**：添加通用 JSON 工具的重载：

```rust
pub fn tools_json(mut self, tools: Vec<serde_json::Value>) -> Self;
```

**收益**：更易于与使用 JSON Schema 的现有工具系统集成。

---

### 1.2 新能力

#### 需求：Provider 指标 API

**需求**：暴露内部指标供外部评分系统使用：

```rust
impl AiClient {
    /// 获取 Provider 性能指标
    pub fn metrics(&self) -> ProviderMetrics {
        ProviderMetrics {
            total_requests: self.stats.total_requests,
            successful_requests: self.stats.successful_requests,
            avg_latency_ms: self.stats.avg_latency_ms,
            total_tokens: self.stats.total_tokens,
            total_cost: self.stats.total_cost,
        }
    }
}
```

**用例**：VelaClaw 的 ProviderScorer 需要实时指标进行自适应模型选择。

---

#### 需求：多模型客户端池

**需求**：从单一接口管理多个 Provider：

```rust
pub struct MultiModelClient {
    clients: HashMap<String, AiClient>,
}

impl MultiModelClient {
    pub fn new(provider_configs: Vec<ProviderConfig>) -> Result<Self>;
    
    /// 获取特定 Provider/模型的客户端
    pub fn client(&self, model_id: &str) -> Option<&AiClient>;
    
    /// 根据条件获取最佳客户端
    pub fn select_best(&self, criteria: SelectionCriteria) -> Option<&AiClient>;
}
```

**用例**：简化 VelaClaw 中的多 Provider 场景。

---

#### 需求：协商辅助器

**需求**：内置多模型响应聚合支持：

```rust
pub struct Negotiator {
    strategy: NegotiationStrategy,
}

impl Negotiator {
    pub fn negotiate(&self, responses: Vec<UnifiedResponse>) -> NegotiationResult;
}

pub enum NegotiationStrategy {
    Voting,
    Consensus,
    BestOfN,
    SelfConsistency,
}
```

**用例**：VelaClaw 的 Agent 循环可以利用内置协商功能。

---

### 1.3 错误处理增强

#### 需求：结构化错误分类

**当前状态**：错误是通用的 `Error` 枚举。

**需求**：添加结构化错误类型以便更好分类：

```rust
pub enum ProviderError {
    RateLimited {
        retry_after: Option<Duration>,
        is_business_error: bool,  // 配额超限、套餐限制
    },
    AuthFailed { reason: String },
    ModelNotFound { model: String },
    ContextWindowExceeded { tokens: u64, limit: u64 },
    Timeout { duration: Duration },
    Network { source: reqwest::Error },
}

impl Error {
    pub fn as_provider_error(&self) -> Option<&ProviderError>;
    pub fn is_retryable(&self) -> bool;
    pub fn retry_after(&self) -> Option<Duration>;
}
```

**用例**：VelaClaw 的 ReliableProvider 可以做出更智能的重试决策。

---

### 1.4 流式改进

#### 需求：带取消句柄的可控流

**当前状态**：`execute_stream()` 返回没有取消功能的流。

**需求**：在流式 API 中暴露取消句柄：

```rust
// 已存在但需要更好的文档
pub async fn execute_stream_with_cancel(
    self,
) -> Result<(Pin<Box<dyn Stream<Item = Result<StreamingEvent>>>>, CancelHandle)>;
```

**用例**：交互式会话中的用户主动取消。

---

## 2. ai-protocol 需求

### 2.1 评分元数据

#### 需求：Provider 质量指标

**需求**：在 Provider YAML 中添加可选的质量/可靠性元数据：

```yaml
id: openai
name: OpenAI
# ... 现有字段 ...

# 新增：评分元数据
scoring:
  baseline_latency_ms: 800
  baseline_cost_per_1k_input: 0.005
  baseline_cost_per_1k_output: 0.015
  reliability_score: 0.99
  quality_score: 0.92
  last_updated: "2024-01-15"
```

**用例**：在发送任何请求之前对 Provider 进行预评分。

---

### 2.2 模型注册增强

#### 需求：扩展模型配置

**需求**：在模型定义中添加任务特定元数据：

```yaml
# v1/models/gpt-4o.yaml
id: gpt-4o
provider: openai
context_window: 128000

# 新增：任务特定能力
capabilities:
  tasks:
    - coding
    - reasoning
    - creative
    - tool_use
    - vision
  supports_parallel_tool_calls: true
  supports_structured_output: true
  supports_vision: true
  
# 新增：性能提示
performance:
  avg_latency_ms: 800
  throughput_tokens_per_sec: 50
  
# 新增：成本信息
pricing:
  input_per_1k: 0.005
  output_per_1k: 0.015
  currency: USD
```

**用例**：无需硬编码配置即可实现更智能的模型选择。

---

### 2.3 协商模式

#### 需求：标准化响应比较

**需求**：定义响应等价性用于投票/共识：

```yaml
# 在 Provider YAML 或单独的 schema 中
negotiation:
  # 如何比较响应是否相等
  equivalence:
    # 规范化空白
    normalize_whitespace: true
    # 比较语义含义（嵌入向量）
    semantic_threshold: 0.95
    # 提取关键答案进行比较
    answer_extraction_pattern: "答案是 (.+)"
```

**用例**：提高基于投票的协商准确性。

---

### 2.4 错误分类规则

#### 需求：Provider 特定的错误映射

**需求**：在协议中定义错误分类：

```yaml
errors:
  mappings:
    - pattern: "rate limit exceeded"
      classification: rate_limited
      retryable: true
      extract_retry_after: "retry after (\\d+)"
      
    - pattern: "insufficient quota"
      classification: rate_limited
      retryable: false
      is_business_error: true
      
    - pattern: "context length exceeded"
      classification: context_exceeded
      retryable: false
      
    - pattern: "invalid api key"
      classification: auth_failed
      retryable: false
```

**用例**：标准化所有 Provider 的错误处理。

---

### 2.5 健康检查端点

#### 需求：Provider 健康配置

**需求**：定义如何检查 Provider 健康状态：

```yaml
health:
  check_endpoint: "/models"
  method: GET
  expected_status: 200
  timeout_ms: 5000
  interval_ms: 30000
  
  # 主检查失败时的备用检查
  fallback:
    method: POST
    endpoint: "/chat/completions"
    body:
      model: "${model_id}"
      messages: [{"role": "user", "content": "ping"}]
      max_tokens: 1
```

**用例**：远程部署健康监控。

---

## 3. 集成建议

### 3.1 功能标志协调

建议在 ai-protocol 中添加功能矩阵文档：

```
| 功能          | ai-lib-rust | VelaClaw | 说明                    |
|--------------|-------------|----------|--------------------------|
| embeddings   | ✅          | ❌       | 需要功能标志            |
| batch        | ✅          | ✅       | 已集成                  |
| negotiation  | ❌          | ✅       | VelaClaw 实现           |
| scoring      | ❌          | ✅       | VelaClaw 实现           |
| multi-model  | ❌          | ✅       | VelaClaw 实现           |
```

### 3.2 共享测试固件

建议在 ai-protocol 中创建共享测试协议文件，用于：
- Mock Provider 响应
- 错误场景
- 边缘情况（空响应、格式错误的 JSON）

### 3.3 版本兼容性

文档化最低所需版本：

```
ai-protocol >= v1.2.0（评分元数据）
ai-lib-rust >= v0.9.0（model() 覆盖）
```

---

## 4. 优先级总结

### 高优先级

| 项目 | 需求 | 影响 |
|------|------|------|
| ai-lib-rust | ChatRequestBuilder 的 `model()` 方法 | 多模型支持关键 |
| ai-lib-rust | AiClient 的 Clone | 流式处理必需 |
| ai-protocol | 错误分类规则 | 提高可靠性 |

### 中优先级

| 项目 | 需求 | 影响 |
|------|------|------|
| ai-lib-rust | Provider 指标 API | 智能路由基础 |
| ai-protocol | 模型任务能力 | 更好的模型选择 |
| ai-protocol | 定价元数据 | 成本感知路由 |

### 低优先级

| 项目 | 需求 | 影响 |
|------|------|------|
| ai-lib-rust | 协商辅助器 | 便利功能 |
| ai-protocol | 健康检查配置 | 部署自动化 |

---

## 5. 拟议贡献

VelaClaw 团队可以回馈：

1. **评分系统**：`ProviderScorer` 实现可以泛化并移至 ai-lib-rust
2. **选择逻辑**：`AdaptiveSelector` 模式可以为 ai-lib-rust 的模型选择提供参考
3. **协商策略**：生产使用验证的协商模式
4. **错误分类**：来自 28+ Provider 的全面错误映射

---

*文档生成自 VelaClaw feat/ai-protocol-integration 分支*
*日期：2026-02-20*
