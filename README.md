# dek

Declarative environment setup. One TOML, any machine.

## Install

```bash
cargo install dek
# or
cargo binstall dek

# setup completions
dek setup
```

## Usage

```bash
dek apply              # apply ./dek.toml or ./dek/
dek check              # dry-run, show what would change
dek plan               # list items (no state check)
dek list               # list available configs
dek run <name>         # run a command from config
dek test               # test in container
dek exec <cmd>         # run command in test container
dek bake               # bake into standalone binary
```

All commands have short aliases: `a`pply, `c`heck, `p`lan, `r`un, `t`est, `dx` (exec).

Config is loaded from: `./dek.toml`, `./dek/`, or `$XDG_CONFIG_HOME/dek/` (fallback).

## Config

```toml
# Packages
[package.os]  # auto-detects: pacman, apt, brew
items = ["curl", "git", "htop"]

[package.apt]
items = ["build-essential"]

[package.pacman]  # falls back to yay for AUR packages
items = ["base-devel", "yay"]

[package.cargo]
items = ["bat", "eza", "ripgrep"]

[package.go]
items = ["github.com/junegunn/fzf@latest"]

[package.npm]
items = ["prettier", "typescript"]

[package.pip]
items = ["httpie", "tldr"]

[package.pipx]
items = ["poetry", "black"]

[package.webi]
items = ["jq", "yq"]

# Systemd services
[[service]]
name = "docker"
state = "active"
enabled = true

# User services (systemctl --user, no sudo)
[[service]]
name = "syncthing"
state = "active"
enabled = true
scope = "user"

# Files
[file.copy]
"dotfiles/.zshrc" = "~/.zshrc"

[file.symlink]
"~/dotfiles/nvim" = "~/.config/nvim"

[file.ensure_line]
"~/.bashrc" = ["export PATH=$HOME/.local/bin:$PATH"]

# Structured line management
[[file.line]]
path = "/etc/needrestart/needrestart.conf"
line = "$nrconf{restart} = 'l';"
original = "#$nrconf{restart} = 'i';"
mode = "replace"

[[file.line]]
path = "/etc/ssh/sshd_config"
line = "PermitRootLogin no"
original_regex = "^#?PermitRootLogin\\s+"
mode = "replace"

# Shell
[alias]
la = "ls -larth"
g = "git"

[env]
EDITOR = "nvim"

# System
timezone = "Europe/Istanbul"
hostname = "workstation"

# Scripts (installed to ~/.local/bin)
[script]
cleanup = "scripts/cleanup.sh"

# Custom commands
[[command]]
name = "setup-db"
check = "psql -c 'SELECT 1 FROM pg_database WHERE datname=mydb'"
apply = "createdb mydb"

# Assertions
[[assert]]
name = "dotty up to date"
check = "git -C ~/dotty fetch -q && test $(git -C ~/dotty rev-list --count HEAD..@{upstream}) -eq 0"
message = "dotty has remote changes"

[[assert]]
name = "note conflicts"
foreach = "rg --files ~/Sync/vault 2>/dev/null | grep conflict | sed 's|.*/||'"

[[assert]]
name = "stow"
foreach = "for p in common nvim tmux; do stow -d ~/dotty -n -v $p 2>&1 | grep -q LINK && echo $p; done"
```

## Assertions

Assertions are check-only items — they report issues but don't change anything. Two modes:

**check** — pass if command exits 0:

```toml
[[assert]]
name = "docker running"
check = "docker info >/dev/null 2>&1"
message = "docker daemon is not running"
stdout = "some regex"  # optional: also match stdout
```

**foreach** — each stdout line is a finding (zero lines = pass):

```toml
[[assert]]
name = "stow packages"
foreach = "for p in common nvim; do stow -n -v $p 2>&1 | grep -q LINK && echo $p; done"
```

In `dek check`, assertions show as `✓`/`✗`. In `dek apply`, failing assertions show as issues (not "changed") and don't block other items.

## Conditional Execution

Any item supports `run_if` — a shell command that gates execution (skip if non-zero):

```toml
[package.pacman]
items = ["base-devel"]
run_if = "command -v pacman"

[[assert]]
name = "desktop stow"
run_if = "echo $(uname -n) | grep -qE 'marko|bender'"
foreach = "..."

[meta]
run_if = "test -d /etc/apt"  # skip entire config file
```

## Package:Binary Syntax

When package and binary names differ:

```toml
[package.cargo]
items = ["ripgrep:rg", "fd-find:fd", "bottom:btm"]
```

Installs `ripgrep`, checks for `rg` in PATH.

## Split Config

```
dek/
├── meta.toml           # project metadata + defaults
├── banner.txt          # optional banner (shown on apply/help)
├── inventory.ini       # remote hosts (one per line)
├── 00-packages.toml
├── 10-services.toml
├── 20-dotfiles.toml
└── optional/
    └── extra.toml      # only applied when explicitly selected
```

Files merged alphabetically. Use `dek apply extra` to include optional configs.

### meta.toml

```toml
name = "myproject"
description = "Project deployment"
version = "1.0"
defaults = ["@setup", "@deploy"]     # default selectors for apply
inventory = "../devops/inventory.ini" # custom inventory path

[test]
image = "ubuntu:22.04"
keep = true
```

### Labels & Selectors

Tag configs with labels for grouped selection:

```toml
# 10-deps.toml
[meta]
name = "Dependencies"
labels = ["setup"]

[package.os]
items = ["curl", "git"]
```

```toml
# 20-deploy.toml
[meta]
name = "Deploy"
labels = ["deploy"]

[file.copy]
"app.jar" = "/opt/app/app.jar"
```

```bash
dek apply @setup          # only configs labeled "setup"
dek apply @deploy         # only configs labeled "deploy"
dek apply @setup tools    # @label refs and config keys can be mixed
dek apply                 # uses meta.toml defaults (or all main configs if no defaults)
```

When `defaults` is set in `meta.toml`, a bare `dek apply` applies only those selectors. Without `defaults`, it applies all non-optional configs (backward compatible).

## Run Commands

Define reusable commands:

```toml
[run.deploy]
description = "Deploy the application"
deps = ["os.rsync"]
cmd = "rsync -av ./dist/ server:/var/www/"

[run.backup]
description = "Backup database"
script = "scripts/backup.sh"  # relative to config dir

[run.restart]
cmd = "systemctl restart myapp"
confirm = true                 # prompt before running

[run.logs]
cmd = "journalctl -fu myapp"
tty = true                     # interactive, uses ssh -t
```

```bash
dek run              # list available commands
dek run deploy       # run command
dek run backup arg1  # args passed via $@
```

### Remote Run

Run commands on remote hosts without deploying dek — just SSH the command directly:

```bash
dek run restart -t server1        # single host
dek run restart -r 'app-*'        # multi-host (parallel)
dek run logs -t server1           # tty command (interactive)
```

- **`-t`** — single host, prints output directly. With `tty: true`, uses `ssh -t` for interactive commands.
- **`-r`** — multi-host from inventory, runs in parallel with progress spinners. `tty: true` commands are rejected (can't attach TTY to multiple hosts).
- **`confirm: true`** — prompts `[y/N]` before running (works both locally and remotely).

## Remote

Apply to remote hosts via SSH:

```bash
dek apply -t user@host
dek check -t server1
```

Use `-q`/`--quiet` to suppress banners (auto-enabled for multi-host). Use `--color always|never|auto` to control colored output.

### Multi-host with Inventory

Ansible-style `inventory.ini` (one host per line, `[groups]` and `;comments` ignored):

```ini
# inventory.ini
[web]
web-01
web-02
web-03

[db]
db-master
```

```bash
dek apply -r 'web-*'    # glob pattern (-r is short for --remotes)
dek apply -r '*'         # all hosts
```

Hosts are deployed in parallel. Override inventory path in `meta.toml`:

```toml
inventory = "../devops/inventory.ini"
```

### Deploy Workflow

Use `[[artifact]]` to build locally before shipping to remotes or baking:

```toml
[[artifact]]
name = "app.jar"
build = "mvn package -DskipTests -q"
watch = ["src", "pom.xml"]              # skip build if unchanged
src = "target/app-1.0.jar"              # build output
dest = "artifacts/app.jar"              # placed in config for shipping

[file.copy]
"artifacts/app.jar" = "/opt/app/app.jar"

[[service]]
name = "app"
state = "active"
```

```bash
dek apply -r 'app-*'
# 1. Builds artifact locally (skips if watch hash unchanged)
# 2. Packages config + artifact into tarball
# 3. Ships to all app-* hosts in parallel
# 4. Copies jar, restarts service

dek bake -o myapp
# Artifact is built and included in the baked binary
```

Artifacts are resolved before any config processing — they work with `apply`, `apply -r`, and `bake`.

Freshness can be determined two ways:
- **`watch`** — list of files/directories to hash (path + size + mtime). Build is skipped when the hash matches the previous run. Best for source trees.
- **`check`** — shell command that exits 0 if the artifact is fresh. Use for custom logic (e.g., `test target/app.jar -nt pom.xml`).

## Inline

Quick installs without a config file:

```bash
dek os.htop os.git cargo.bat
dek pip.httpie npm.prettier
```

## Test

Bakes config into the binary and runs it in a container. The baked `dek` inside the container is fully functional — `apply`, `list`, `run` all work.

```bash
dek test                     # bake + create container + apply + shell
dek test @core               # only apply @core labeled configs
dek test -i ubuntu tools     # custom image + specific configs
dek test                     # (second run) rebake + apply + shell (container kept)
dek test -f                  # force new container (remove + recreate)
dek test -a                  # attach to running container (no rebuild)
dek test -r                  # remove container after exit
```

Containers are kept by default and named `dek-test-{name}` (from `meta.toml` name or directory). On subsequent runs, dek rebakes the binary, copies it into the existing container, reapplies config, and drops into a shell — installed packages and files persist.

### Exec

Run commands directly in the test container:

```bash
dek exec ls /opt/app         # run a command
dek dx cat /etc/os-release   # dx is a short alias
dek dx dek run version       # run dek commands inside
dek dx dek list              # list configs in container
```

Configure defaults in `meta.toml`:

```toml
[test]
image = "ubuntu:22.04"
```

CLI flags override meta.toml (`-i/--image`, `-r/--rm`).

## Bake

Embed config into a standalone binary:

```bash
dek bake ./dek -o mysetup
./mysetup              # show help with available configs
./mysetup apply        # apply all
./mysetup run deploy   # run commands
```
