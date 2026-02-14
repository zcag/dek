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
dek run <name>         # run a command from config
dek state              # query system state probes
dek test               # test in container
dek exec <cmd>         # run command in test container
dek bake               # bake into standalone binary
```

All commands have short aliases: `a`pply, `c`heck, `p`lan, `r`un, `s`tate, `t`est, `dx` (exec).

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

[file.fetch]
"https://raw.githubusercontent.com/user/repo/main/.bashrc" = "~/.bashrc"
"https://example.com/config.json" = { path = "~/.config/app/config.json", ttl = "1h" }

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

## File Fetch

Download files from URLs. Results are cached at `~/.cache/dek/url/`. Use `ttl` to control cache expiry:

```toml
[file.fetch]
# Cache forever (re-fetches only when cache is cleared)
"https://raw.githubusercontent.com/user/repo/main/.bashrc" = "~/.bashrc"

# Cache for 1 hour — re-fetches if older
"https://example.com/config.json" = { path = "~/.config/app/config.json", ttl = "1h" }
```

Supported TTL units: `s` (seconds), `m` (minutes), `h` (hours), `d` (days). Can be combined: `1h30m`.

## Vars

Runtime variables defined in `meta.toml`, set in the process environment before anything runs. Available to all providers, commands, scripts — locally and remotely.

```toml
# meta.toml
[vars]
APP_NAME = "myapp"
DEPLOY_DIR = "/opt/default"

# Scoped by @label — applied when that label is selected
[vars."@staging"]
DEPLOY_DIR = "/opt/staging"
DB_HOST = "staging-db"

[vars."@production"]
DEPLOY_DIR = "/opt/production"
DB_HOST = "prod-db"

# Scoped by config key
[vars.deploy]
NOTIFY = "true"
```

Base vars are always set. Scoped vars overlay when their selector is active:

```bash
dek apply @staging    # APP_NAME=myapp DEPLOY_DIR=/opt/staging DB_HOST=staging-db
dek apply @production # APP_NAME=myapp DEPLOY_DIR=/opt/production DB_HOST=prod-db
dek apply deploy      # APP_NAME=myapp DEPLOY_DIR=/opt/default NOTIFY=true
```

Vars are inherited by all child processes, so `[[command]]` check/apply, `[script]`, and remote `dek apply` all see them.

## Cache Key

Skip steps when a value hasn't changed since last successful apply. Works on `[[command]]`, `[[service]]`, and `[[file.line]]`.

**`cache_key`** — a string value (supports `$VAR` expansion):

```toml
[[command]]
name = "generate test data"
check = "test -f /opt/test/input.csv"
apply = "generate-data.sh"
cache_key = "$INPUT_FILE_SIZE_MB"  # only re-runs when size changes
```

**`cache_key_cmd`** — a command whose stdout is the cache key:

```toml
[[command]]
name = "deploy jar"
check = "test -f /opt/dpi/jar/dpi.jar"
apply = "cp build/dpi.jar /opt/dpi/jar/"
cache_key_cmd = "sha256sum build/dpi.jar"  # only re-deploys when jar changes
```

Cache state is stored in `~/.cache/dek/state/`. The provider's `check` always runs — if the state is missing (e.g. file deleted), apply runs regardless of cache. When check passes and the cache key is unchanged, apply is skipped. When the cache key changes (e.g. a `$VAR` in `meta.toml` was updated), apply re-runs even if check still passes — this lets you force re-apply by changing a var.

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
min_version = "0.1.28"               # auto-update dek if older
defaults = ["@setup", "@deploy"]     # default selectors for apply
inventory = "../devops/inventory.ini" # custom inventory path
remote_install = true                # symlink dek + config on remote hosts

[test]
image = "ubuntu:22.04"
keep = true
mount = ["./data:/opt/data"]         # bind mounts for test container
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
- **Vars** — base vars from `meta.toml` `[vars]` are exported to the remote shell automatically, so `$VAR` references in remote commands resolve correctly.

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

### Remote Install

With `remote_install = true` in `meta.toml`, dek symlinks itself and the config on remote hosts after deploy:

```
~/.local/bin/dek → /tmp/dek-remote/dek
~/.config/dek   → /tmp/dek-remote/config/
```

This lets you run `dek` directly on the remote (e.g. `dek run`, `dek list`) without re-deploying.

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

**`deps`** — local dependencies needed before build. Ensures build tools exist on the machine running the build:

```toml
[[artifact]]
name = "app.jar"
build = "mvn package -DskipTests -q"
deps = ["apt.default-jdk:java", "apt.maven:mvn"]
src = "target/app-1.0.jar"
dest = "artifacts/app.jar"
```

Format: `"package:binary"` — installs `package` if `binary` isn't in PATH. Prefix with package manager (`apt.`, `pacman.`, `brew.`) to force a specific one, or omit for auto-detection (`os.`).

## Auto Update

Set `min_version` in `meta.toml` to ensure all users/hosts run a compatible version:

```toml
min_version = "0.1.28"
```

If the running dek is older, it auto-updates via `cargo-binstall` (preferred) or `cargo install`, then exits with a prompt to rerun.

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
mount = ["./data:/opt/data", "/host/path:/container/path"]
```

Mounts are bind-mounted into the test container. Relative host paths are resolved against the config directory.

CLI flags override meta.toml (`-i/--image`, `-r/--rm`).

## Completions

Dynamic completions for configs, @labels, and run commands.

### Manual

```bash
dek setup              # auto-detect shell, install completions
dek completions zsh    # raw output (pipe to file yourself)
```

### Via dek config

Add to any config (e.g., `15-shell.toml`):

```toml
[[command]]
name = "dek completions"
cmd = "dek setup"
check = "dek _complete check"
```

Completions support all aliases (`a`, `c`, `p`, `r`, `t`, `dx`) and dynamically complete config keys, `@labels`, and run command names from whatever config is in the current directory.

## State

Query system state via shell commands with optional rewrite rules. Probes run in parallel.

```toml
[[state]]
name = "machine"
cmd = "uname -n"

[[state]]
name = "screen"
cmd = "hyprctl -j monitors | jq -r '.[].description'"
rewrite = [
  {match = "Samsung.*0x01000E00", value = "tv"},
  {match = "C49RG9x", value = "ultrawide"},
]

[[state]]
name = "hour"
cmd = "date +%H"
rewrite = [
  {match = "^(2[0-3]|0[0-7])$", value = "night"},
  {match = ".*", value = "day"},
]
```

Rewrite rules are checked in order against raw stdout. First regex match wins, output replaced with `value`. No match = raw output.

```bash
dek state                          # all probes, aligned key/value
dek state --json                   # all probes as JSON object
dek state machine                  # single probe value
dek state machine screen hour      # multiple probes
dek state machine --json           # {"machine":"marko"}
dek state networktype is ethernet  # exit 0 if match, 1 otherwise
dek state networktype isnot wifi   # exit 0 if NOT match, 1 otherwise
dek state screen get tv ultra def  # "tv"/"ultra" pass through, else "def"
```

Alias: `s`. Useful in scripts:

```bash
dek s machine is marko && hyprctl dispatch ...
dek s hour is night && notify-send "go to sleep"
theme=$(dek s screen get tv ultrawide default)
```

## Bake

Embed config into a standalone binary:

```bash
dek bake ./dek -o mysetup
./mysetup              # show help with available configs
./mysetup apply        # apply all
./mysetup run deploy   # run commands
```
