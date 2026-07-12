# Operations & Deployment Docs

For operators running VelaClaw in persistent or production-like environments.

## Core Operations

- Day-2 runbook: [../operations-runbook.md](../operations-runbook.md)
- Config vs rebuild contract: [../config-externalization.md](../config-externalization.md)
- Release runbook: [../release-process.md](../release-process.md)
- Troubleshooting matrix: [../troubleshooting.md](../troubleshooting.md)
- Safe network/gateway deployment: [../network-deployment.md](../network-deployment.md)
- Mattermost setup (channel-specific): [../mattermost-setup.md](../mattermost-setup.md)

## Common Flow

1. Validate runtime (`status`, `doctor`, `channel doctor`)
2. Apply one config change at a time (see [config-externalization.md](../config-externalization.md))
3. Prefer hot-reload for provider/model/`max_tool_iterations`; restart only when required
4. Verify channel and gateway health
5. Roll back quickly if behavior regresses

## Related

- Config reference: [../config-reference.md](../config-reference.md)
- Security collection: [../security/README.md](../security/README.md)
