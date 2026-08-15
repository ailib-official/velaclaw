# Upgrade runbook — VelaClaw 1.0.x

How to upgrade an existing install without losing secrets or trial config.

Last verified: **2026-08-15** (1.0.3 patch train).

## Non-negotiables

| Rule | Detail |
|------|--------|
| Keep config dir | Respect `VELACLAW_CONFIG_DIR` (trial example: `~/.velaclaw-trial`). Do not switch back to `~/.velaclaw` by accident. |
| Never overwrite secrets | Do **not** replace `daemon.env` / `*.env` with templates. Redeploy binary + unit only. |
| Protocol pin | Keep `AI_PROTOCOL_DIR` pointing at the protocol checkout expected by this release train. |
| SemVer 1.0.x | Cargo/CHANGELOG track the train; GitHub Release tags use `vX.Y.Z` via [release-process.md](release-process.md). |

## Recommended binary upgrade (local / trial)

```bash
# 1) Confirm what is running
velaclaw --version
curl -sS http://127.0.0.1:3000/health | jq '{version,status}'

# 2) Build from main (or a release tag)
cd /path/to/velaclaw
git fetch origin && git checkout main && git pull --ff-only origin main
# Optional: checkout the release tag after it is cut, e.g. git checkout v1.0.3
# Embed current Web Chat: ui-chat/dist is gitignored; skip this and /chat stays stale.
(cd ui-chat && npm ci && npm run build)
cargo build --release --features ai-protocol
install -m 755 target/release/velaclaw ~/.local/bin/velaclaw   # or your install path

# 3) Restart service — do not touch daemon.env
systemctl --user daemon-reload
systemctl --user restart velaclaw.service   # unit name may differ

# 4) Re-verify identity
velaclaw --version
curl -sS http://127.0.0.1:3000/health | jq '{version,status}'
# CLI --version and /health.version must match (same installed binary).
```

## Config / secrets checklist

- [ ] `VELACLAW_CONFIG_DIR` unchanged
- [ ] `config.toml` not replaced by a scaffold (merge profiles like `examples/profiles/ops-readonly.toml` only when intended)
- [ ] `daemon.env` mode stays `600`; contents untouched
- [ ] `AI_PROTOCOL_DIR` still resolves
- [ ] After restart: `velaclaw doctor` / channel doctor as needed

## Fresh install (not upgrade)

Use [one-click-bootstrap.md](one-click-bootstrap.md) or `velaclaw onboard`. Fresh onboard may seed workspace `agent-policy.yaml`; it does not invent `daemon.env` secrets for an existing trial.

## CI merge discipline (operators / bots)

- Required merge gate: **CI Required Gate** (see [ci-map.md](ci-map.md)).
- Cron / review bots that merge on green CI must be **fail-closed**: isolate log stdout from CI status parsing; unknown or red CI must **not** merge. Product reference: [ci-map.md#merge-bot-fail-closed](ci-map.md#merge-bot-fail-closed).

## Related

- Day-2 ops: [operations-runbook.md](operations-runbook.md)
- Maintainer tag/publish: [release-process.md](release-process.md)
- CHANGELOG: [../CHANGELOG.md](../CHANGELOG.md)
