# USER

- Prefers concise, direct answers
- Often mixes Chinese and English
- After tools run, wants a brief conclusion — not a second copy of the tool output

## Local infrastructure (index)

> **Customize** this table to match your `~/.ssh/config`. Hostnames are **SSH aliases**, not local folders.
> Longer notes: `INFRA.md` (not auto-injected today — keep critical facts here).

| Name | Role | Access |
|------|------|--------|
| `workstation` | Dev terminal | local |
| `piubt` | LAN proxy / light CI / tools (`192.168.2.13`) | `ssh piubt` |
| `git-server` / `lan-git` | Bare-git SoT (`192.168.2.22`), repos in `/srv/git/repos` | `ssh lan-git` |
| `eos-hk` | Cloud VPS / public edge | `ssh eos-hk` |

If the user names one of these hosts, act on **that machine via SSH**.
The LAN git box is bare git + SSH — **not** Gitea (unless you intentionally run one).
