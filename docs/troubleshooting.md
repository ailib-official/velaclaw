# VelaClaw Troubleshooting

This guide focuses on common setup/runtime failures and fast resolution paths.

Last verified: **February 20, 2026**.

## Installation / Bootstrap

### NVIDIA NIM agent returns HTTP 404

Symptoms:

- Bare `curl` to `https://integrate.api.nvidia.com/v1/chat/completions` succeeds for a model
- `velaclaw agent -p nvidia --model …` fails with HTTP 404, or remaps to OpenAI unexpectedly

Checks:

1. Prefer `-p nvidia --model <catalog-id>` or `--model nvidia/<catalog-id>` (VL-RT-004). A bare
   `--model meta/…` without `-p nvidia` is treated as provider `meta`.
2. For **nvidia-org** catalog ids (`nvidia/nemotron-…`, not `nvidia/meta/…`), confirm the host
   expands the BYOK init id so the wire `model` keeps the `nvidia/` prefix (VL-RT-005 / E5c).
   Bare wire `nemotron-mini-4b-instruct` → `404 page not found`; correct wire is
   `nvidia/nemotron-mini-4b-instruct`.
3. Confirm `NVIDIA_API_KEY` is set and the model is enabled for the account
   (`Not found for account` on entitlement-gated ids such as
   `nvidia/nemotron-4-340b-instruct` is an account miss, not a missing host feature;
   fresh installs default to `nvidia/nemotron-mini-4b-instruct`).
4. Use `velaclaw doctor routing` to see configured vs effective logical model (no secret values).

See [providers-reference.md](providers-reference.md#nvidia-nim-notes).

### `cargo` not found

Symptom:

- bootstrap exits with `cargo is not installed`

Fix:

```bash
./bootstrap.sh --install-rust
```

Or install from <https://rustup.rs/>.

### Missing system build dependencies

Symptom:

- build fails due to compiler or `pkg-config` issues

Fix:

```bash
./bootstrap.sh --install-system-deps
```

### Build fails on low-RAM / low-disk hosts

Symptoms:

- `cargo build --release` is killed (`signal: 9`, OOM killer, or `cannot allocate memory`)
- Build crashes after adding swap because disk space runs out

Why this happens:

- Runtime memory (<5MB for common operations) is not the same as compile-time memory.
- Full source build can require **2 GB RAM + swap** and **6+ GB free disk**.
- Enabling swap on a tiny disk can avoid RAM OOM but still fail due to disk exhaustion.

Preferred path for constrained machines:

```bash
./bootstrap.sh --prefer-prebuilt
```

Binary-only mode (no source fallback):

```bash
./bootstrap.sh --prebuilt-only
```

If you must compile from source on constrained hosts:

1. Add swap only if you also have enough free disk for both swap + build output.
1. Limit cargo parallelism:

```bash
CARGO_BUILD_JOBS=1 cargo build --release --locked
```

1. Reduce heavy features when Matrix is not required:

```bash
cargo build --release --locked --features hardware
```

1. Cross-compile on a stronger machine and copy the binary to the target host.

### Build is very slow or appears stuck

Symptoms:

- `cargo check` / `cargo build` appears stuck at `Checking velaclaw` for a long time
- repeated `Blocking waiting for file lock on package cache` or `build directory`

Why this happens in VelaClaw:

- Matrix E2EE stack (`matrix-sdk`, `ruma`, `vodozemac`) is large and expensive to type-check.
- TLS + crypto native build scripts (`aws-lc-sys`, `ring`) add noticeable compile time.
- `rusqlite` with bundled SQLite compiles C code locally.
- Running multiple cargo jobs/worktrees in parallel causes lock contention.

Fast checks:

```bash
cargo check --timings
cargo tree -d
```

The timing report is written to `target/cargo-timings/cargo-timing.html`.

Faster local iteration (when Matrix channel is not needed):

```bash
cargo check
```

This uses the lean default feature set and can significantly reduce compile time.

To build with Matrix support explicitly enabled:

```bash
cargo check --features channel-matrix
```

To build with Matrix + Lark + hardware support:

```bash
cargo check --features hardware,channel-matrix,channel-lark
```

Lock-contention mitigation:

```bash
pgrep -af "cargo (check|build|test)|cargo check|cargo build|cargo test"
```

Stop unrelated cargo jobs before running your own build.

### `velaclaw` command not found after install

Symptom:

- install succeeds but shell cannot find `velaclaw`

Fix:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
which velaclaw
```

Persist in your shell profile if needed.

### Wrong / stale `velaclaw` on PATH (VL-OPS-001)

Symptoms:

- `velaclaw --version` or behavior does not match the binary you just built/installed
- `~/bin/velaclaw` is current but `~/.local/bin/velaclaw` (sometimes root-owned) shadows it

Checks (observe-only; does not rewrite PATH):

```bash
velaclaw doctor maintenance
which -a velaclaw
```

Prefer a single install location early on `PATH` (common: `$HOME/bin`). Compare
`this_process` vs `first_on_PATH` in the maintenance guide. Do not `sudo` remove
binaries unless you understand ownership — reinstall or adjust `PATH` order.

### L4 shadow M3 fields with no Grafana (CR-HOST-002)

Symptoms:

- Need pass / schema-fail / L4→L2 fallback counts from local shadow traffic
- No Prometheus/Grafana available (and none required)

Checks (observe-only; does **not** enable default-on L4):

```bash
RUST_LOG=info velaclaw doctor candidate-dag --candidate <path> 2>shadow.log
velaclaw doctor l4-shadow-summary --log shadow.log
# or: … --json
```

Fields aggregated: `m3c_pass`, `m3d_category`, `m3e_fallback` on
`candidate_dag_*` events. `[agent].candidate_dag_shadow` remains default `false`.

## Runtime / Gateway

### Gateway unreachable

Checks:

```bash
velaclaw status
velaclaw doctor
```

Verify `~/.velaclaw/config.toml`:

- `[gateway].host` (default `127.0.0.1`)
- `[gateway].port` (default `3000`)
- `allow_public_bind` only when intentionally exposing LAN/public interfaces

### Pairing / auth failures on webhook

Checks:

1. Ensure pairing completed (`/pair` flow)
2. Ensure bearer token is current
3. Re-run diagnostics:

```bash
velaclaw doctor
```

## Channel Issues

### Telegram conflict: `terminated by other getUpdates request`

Cause:

- multiple pollers using same bot token

Fix:

- keep only one active runtime for that token
- stop extra `velaclaw daemon` / `velaclaw channel start` processes

### Channel unhealthy in `channel doctor`

Checks:

```bash
velaclaw channel doctor
```

Then verify channel-specific credentials + allowlist fields in config.

## Service Mode

### Service installed but not running

Checks:

```bash
velaclaw service status
```

Recovery:

```bash
velaclaw service stop
velaclaw service start
```

Linux logs:

```bash
journalctl --user -u velaclaw.service -f
```

## Legacy Installer Compatibility

Both still work:

```bash
curl -fsSL https://raw.githubusercontent.com/velaclaw-labs/velaclaw/main/scripts/bootstrap.sh | bash
curl -fsSL https://raw.githubusercontent.com/velaclaw-labs/velaclaw/main/scripts/install.sh | bash
```

`install.sh` is a compatibility entry and forwards/falls back to bootstrap behavior.

## Policy / approval (0.7.0+)

### Supervised tools denied on channels

Symptoms:

- Agent replies with denial when requesting `shell` or other gated tools on Telegram/Discord
- Works in CLI but not in channel

Checks:

1. `[autonomy].level` — `supervised` requires human approval.
2. Channel `approval_mode` — `deny` blocks interactive approval; use `inline` for in-chat prompts.
3. `approval_timeout_secs` — expired prompts deny the call.
4. Operator responded in time and used Y/N/A (not plain chat).

See [policy-approval-reference.md](policy-approval-reference.md).

### Shell fails after upgrade (no `approved` parameter)

Symptoms:

- Model attempts `shell` with `approved: true` but execution still denied

Fix:

- Remove system prompts that tell the model to self-approve.
- Approve via CLI stdin, gateway Web UI, or channel inline prompt.
- See [migration-policy-v0.7.0.md](migration-policy-v0.7.0.md).

### Shell refused after upgrade (sandbox fail-closed)

Symptoms:

- Allowlisted commands fail with `Sandbox wrap failed` / fail-closed
- `velaclaw doctor` shows `sandbox=fail-closed`

Cause:

- Linux Auto now refuses shell when Landlock is unavailable (no silent Noop).
- Non-Linux Auto is still Noop; this symptom is Linux-specific.

Fix (pick one):

1. Use a kernel with Landlock (typically 5.13+) and a default-feature build (`sandbox-landlock`).
2. Intentional YOLO: `[security.sandbox] enabled = false` and/or `backend = "none"`, then confirm doctor shows `source=explicit_yolo`.

See [config-reference.md](config-reference.md#securitysandbox).

### `sudo` / `apt` fails under Landlock (no-new-privileges)

Symptoms:

- `sudo: The "no new privileges" flag is set…`
- Package managers cannot write under `/var` despite allowlist

Cause:

- Default Landlock wrap sets `PR_SET_NO_NEW_PRIVS` on the shell child (policy A).
- Approval alone does not remove the OS sandbox.

Fix (opt-in policy B — power users only):

1. Set `[security.sandbox] escape_on_approval = true`.
2. Keep `sudo` / `apt` in `allowed_commands`.
3. Approve the shell-policy prompt in Web/CLI; that invocation skips Landlock.
4. Prefer `sudo -S` + `request_human_input` secret_slot over embedding passwords.

Do **not** set `backend = "none"` unless you intentionally want YOLO for every shell.

### Session allowlist lost after restart (pre-0.7)

On **0.7.0+**, **Always** decisions persist to `<workspace>/.velaclaw/policy-overrides.yaml`. Verify the file exists and `[security.audit]` / workspace path is correct.

## Still Stuck?

Collect and include these outputs when filing an issue:

```bash
velaclaw --version
velaclaw status
velaclaw doctor
velaclaw channel doctor
```

Also include OS, install method, and sanitized config snippets (no secrets).

## Related Docs

- [operations-runbook.md](operations-runbook.md)
- [one-click-bootstrap.md](one-click-bootstrap.md)
- [channels-reference.md](channels-reference.md)
- [network-deployment.md](network-deployment.md)
