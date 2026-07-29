# INFRA — Host index (example)

Short index of known machines. Hostnames are **SSH Host aliases** (see `~/.ssh/config`), not local directories.

> **Not auto-injected** by VelaClaw today. Mirror critical rows into `USER.md`. Customize before use.

| Name | Where | Role | Access |
|------|-------|------|--------|
| `workstation` | local | Dev terminal | local |
| `piubt` | LAN `192.168.2.13` | Proxy, light CI, tools | `ssh piubt` |
| `git-server` / `lan-git` | LAN `192.168.2.22` | Private bare-git SoT | `ssh lan-git` (preferred) or `ssh git-server` |
| `eos-hk` | Cloud VPS | Public edge / app hosts | `ssh eos-hk` |

## git-server notes (example)
- SSH user is often `git` (not root). Prefer alias **`lan-git`** if configured.
- Source of truth may be **bare repos** under `/srv/git/repos/*.git` — not a forge UI.
- Do not assume a `gitea` unit exists; list repos with `ls /srv/git/repos` (or your path).

## Conventions
- If the user names a host from this table, treat it as **that machine** (SSH), never as a local folder under this workspace.
- Typical remote listing: `ssh -o BatchMode=yes <host> 'ls /'`
- Secrets stay in SSH keys / env — not here.
