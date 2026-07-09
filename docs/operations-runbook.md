# VelaClaw Operations Runbook

This runbook is for operators who maintain availability, security posture, and incident response.

Last verified: **February 18, 2026**.

## Scope

Use this document for day-2 operations:

- starting and supervising runtime
- health checks and diagnostics
- safe rollout and rollback
- incident triage and recovery

For first-time installation, start from [one-click-bootstrap.md](one-click-bootstrap.md).

## Runtime Modes

| Mode | Command | When to use |
|---|---|---|
| Foreground runtime | `velaclaw daemon` | local debugging, short-lived sessions |
| Foreground gateway only | `velaclaw gateway` | webhook endpoint testing |
| User service | `velaclaw service install && velaclaw service start` | persistent operator-managed runtime |

## Baseline Operator Checklist

1. Validate configuration:

```bash
velaclaw status
```

2. Verify diagnostics:

```bash
velaclaw doctor
velaclaw channel doctor
```

3. Start runtime:

```bash
velaclaw daemon
```

4. For persistent user session service:

```bash
velaclaw service install
velaclaw service start
velaclaw service status
```

## Health and State Signals

| Signal | Command / File | Expected |
|---|---|---|
| Config validity | `velaclaw doctor` | no critical errors |
| Channel connectivity | `velaclaw channel doctor` | configured channels healthy |
| Runtime summary | `velaclaw status` | expected provider/model/channels |
| Daemon heartbeat/state | `~/.velaclaw/daemon_state.json` | file updates periodically |

## Logs and Diagnostics

### macOS / Windows (service wrapper logs)

- `~/.velaclaw/logs/daemon.stdout.log`
- `~/.velaclaw/logs/daemon.stderr.log`

### Linux (systemd user service)

```bash
journalctl --user -u velaclaw.service -f
```

## Incident Triage Flow (Fast Path)

1. Snapshot system state:

```bash
velaclaw status
velaclaw doctor
velaclaw channel doctor
```

2. Check service state:

```bash
velaclaw service status
```

3. If service is unhealthy, restart cleanly:

```bash
velaclaw service stop
velaclaw service start
```

4. If channels still fail, verify allowlists and credentials in `~/.velaclaw/config.toml`.

5. If gateway is involved, verify bind/auth settings (`[gateway]`) and local reachability.

## Safe Change Procedure

Before applying config changes:

1. backup `~/.velaclaw/config.toml`
2. apply one logical change at a time
3. run `velaclaw doctor`
4. restart daemon/service
5. verify with `status` + `channel doctor`

## Rollback Procedure

If a rollout regresses behavior:

1. restore previous `config.toml`
2. restart runtime (`daemon` or `service`)
3. confirm recovery via `doctor` and channel health checks
4. document incident root cause and mitigation

## Related Docs

- [one-click-bootstrap.md](one-click-bootstrap.md)
- [troubleshooting.md](troubleshooting.md)
- [config-reference.md](config-reference.md)
- [commands-reference.md](commands-reference.md)
- [policy-approval-reference.md](policy-approval-reference.md)
- [migration-policy-v0.7.0.md](migration-policy-v0.7.0.md)
