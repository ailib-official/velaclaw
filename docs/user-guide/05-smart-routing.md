# 第五章：模型路由

> **VL-DOC-003：** 历史上的 `velaclaw agent --smart`、`velaclaw daemon --smart`、
> Cargo feature `smart-routing` **均已删除**。请勿再按旧教程编译或传参。

当前合同（CLI / Web）：

1. 显式 `--provider` / `--model` 或 Web 选择器
2. 可选 `[agent].host_decide`
3. 可选 `[agent].intent_capability_route`（Tag/Hint → 可达模型 ∩ `[[model_routes]]`）
4. `[[model_routes]]` / `query_classification` / `default_model`

真源：[config-reference.md](../config-reference.md#agent)（TOML，不是 `config.yaml`）。

```toml
[[model_routes]]
hint = "code"
provider = "deepseek"
model = "deepseek-chat"
```

观察：`velaclaw doctor routing`、`velaclaw doctor capability-route --tag <Tag> --force`。
