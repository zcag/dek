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
bin_name = "mytool"                  # binary symlink name on remote (default: "dek")

# Hide sections from the welcome screen
# values: "usage", "commands", "options", "configs", "run", "powered", "powered_url"
hide = ["commands", "options", "usage", "powered_url"]

# Custom entries shown as COMMANDS section on the welcome screen
[[welcome]]
name = "deploy"
description = "Deploy to production"

[[welcome]]
name = "status"
description = "Check service status"

[test]
image = "ubuntu:22.04"
keep = true
mount = ["./data:/opt/data"]         # bind mounts for test container
```

When `name` is set, the welcome screen shows a "Powered by dek" line — useful for branded tools deployed via `remote_install`.

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

## Shell Library

Put shared shell functions in `data/functions.sh` under your config directory. dek automatically sources it before every `cmd`, `check`, `apply`, `run`, and `assert` script:

```
~/.config/dek/
  data/
    functions.sh   ← sourced automatically
  10-tools.toml
  20-apps.toml
```

```bash
# data/functions.sh
is_laptop() { [ "$(uname -n)" = "thinkpad" ]; }
has_display() { [ -n "$DISPLAY" ] || [ -n "$WAYLAND_DISPLAY" ]; }
```

```toml
[[command]]
name = "laptop-brightness"
check = "is_laptop && has_display && cat /sys/class/backlight/*/brightness | grep -q ."
apply = "is_laptop && brightnessctl set 50%"

[[state]]
name = "laptop"
cmd = "is_laptop && echo yes || echo no"
```

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
~/.cache/dek/remote/dek      ← binary (persists across reboots)
~/.cache/dek/remote/config/  ← config

~/.config/dek          → ~/.cache/dek/remote/config/
/usr/local/bin/dek     → ~/.cache/dek/remote/dek   (root)
~/.local/bin/dek       → ~/.cache/dek/remote/dek   (non-root)
```

Use `bin_name` to deploy as a custom-named tool:

```toml
# meta.toml
name = "recover"           # display name (shown in welcome screen)
remote_install = true
bin_name = "recover"       # binary symlink name on remote
```

After deploy: `recover apply`, `recover run logs`, etc.

This lets you run `dek` directly on the remote (e.g. `dek apply`, `dek run`) without re-deploying. Re-deploying updates the cached binary and config in-place, so the symlinks stay valid.

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

Query system state via shell commands with optional rewrite rules, named templates, and dependencies. Probes run in parallel (respecting dependency order).

```toml
[[state]]
name = "machine"
cmd = "uname -n"

[[state]]
name = "screen"
cmd = "hyprctl -j monitors | jq -r '.[].description'"
ttl = "1h"   # cache probe output for 1 hour
rewrite = [
  {match = "Samsung.*0x01000E00", value = "tv"},
  {match = "C49RG9x", value = "ultrawide"},
]
templates = { short = "{{ raw[:2] }}", icon = "{% if raw == 'tv' %}T{% else %}U{% endif %}" }

[[state]]
name = "hour"
cmd = "date +%H"
rewrite = [
  {match = "^(2[0-3]|0[0-7])$", value = "night"},
  {match = ".*", value = "day"},
]

# Computed state — no cmd, just deps + templates
[[state]]
name = "summary"
deps = ["machine", "screen", "hour"]
templates = { default = "{{ machine.raw }}/{{ screen.raw }}/{{ hour.raw }}" }
```

Rewrite rules are checked in order against raw stdout. First regex match wins, output replaced with `value`. No match = raw output. When a rewrite matches, the pre-rewrite value is preserved as `original`.

### Templates

Named Jinja templates rendered after cmd+rewrite. Context includes `raw`, `original`, and all dependency values (`dep.raw`, `dep.original`, `dep.<template>`).

The `fromjson` filter parses JSON strings into objects for field access:

```toml
[[state]]
name = "weather"
cmd = "curl -s 'https://api.example.com/weather'"
ttl = "30m"
templates.short = "{{ (raw | fromjson).text }}"
templates.long = "{{ (raw | fromjson).tooltip }}"
```

### TTL

Cache slow probe commands so they don't re-run every time. Cached output is stored in `~/.cache/dek/url/` and reused until the TTL expires. The raw command output is cached (before rewrites/templates), so rewrites and templates always re-evaluate.

```toml
[[state]]
name = "screen"
cmd = "hyprctl -j monitors | jq -r '.[].description'"
ttl = "1h"   # re-run cmd only after 1 hour
```

No `ttl` = no caching (runs every time). Supported units: `s`, `m`, `h`, `d` (combinable: `1h30m`).

### Expressions

`expr` is a Jinja template rendered with dependency values to produce the raw value — an alternative to `cmd` for computed states. Rewrites apply to the result, so you can combine deps into a matchable string:

```toml
[[state]]
name = "network"
deps = ["machine", "ssid", "networktype"]
expr = "{{ machine.raw }}:{{ networktype.raw }}:{{ ssid.raw }}"
rewrite = [
  {match = "marko:ethernet", value = "home"},
  {match = "bender:ethernet", value = "ng"},
  {match = ".*:wifi:home", value = "home"},
  {match = ".*:wifi:office", value = "ng"},
  {match = ".*", value = "other"},
]
```

### Dependencies

States can depend on other states via `deps`. Dependencies are evaluated first (topologically sorted), and their results are available in templates. States without `cmd` are computed purely from deps+templates.

```bash
dek state                          # all probes, aligned key/value
dek state --json                   # nested JSON: {"screen":{"raw":"tv","original":"Samsung...","icon":"T"}}
dek state machine                  # single probe value
dek state screen.icon              # named template value
dek state screen.original          # pre-rewrite value
dek state machine screen hour      # multiple probes
dek state screen.icon is T         # operators work on any variant
dek state screen get tv ultra def  # "tv"/"ultra" pass through, else "def"
dek state summary.default          # computed from deps: "hostname/tv/night"
```

Alias: `s`. Useful in scripts:

```bash
dek s machine is marko && hyprctl dispatch ...
dek s hour is night && notify-send "go to sleep"
theme=$(dek s screen get tv ultrawide default)
icon=$(dek s screen.icon)
```

## File Templates

Render Jinja template files with state values, built-in variables, and vars files. Templates are checked/applied like any other file provider.

```toml
[[state]]
name = "screen"
cmd = "hyprctl -j monitors | jq -r '.[].description'"
ttl = "1h"
rewrite = [{match = "Samsung.*", value = "tv"}]
templates = { icon = "{% if raw == 'tv' %}T{% else %}U{% endif %}" }

[[state]]
name = "hour"
cmd = "date +%H"

[[file.template]]
src = "templates/waybar.json.j2"
dest = "~/.config/waybar/config.json"
states = ["screen", "hour"]
```

`templates/waybar.json.j2`:
```
// Generated on {{ hostname }} by {{ user }}
{
  "output": "{{ screen.raw }}",
  "icon": "{{ screen.icon }}",
  "mode": "{{ hour.raw }}"
}
```

### Template Context

**Built-ins** (always available): `hostname`, `user`, `os`, `arch`

**States** (from `states` field): each state is an object with `.raw`, `.original`, and any template variant keys (e.g. `screen.icon`).

Only states listed in `states` (and their transitive dependencies) are evaluated.

Missing variables render as empty strings (lenient mode).

### Vars Files

Load external variable files (YAML or TOML) into the template context — like Ansible's vars files. Supports nested maps, arrays, and complex structures.

**Shared vars** — available to all templates:

```toml
[file]
vars = ["vars/common.yaml", "vars/defaults.toml"]
```

**Per-template vars** — merged on top of shared vars (overrides):

```toml
[[file.template]]
src = "templates/app.conf.j2"
dest = "~/.config/app/config"
vars = ["vars/site.yaml"]
states = ["screen"]
```

File format is detected by extension: `.yaml`/`.yml` for YAML, `.toml` for TOML. All top-level keys become template variables.

Example `vars/site.yaml`:
```yaml
site_vars:
  kafka_server:
    - 169.254.0.10:9092
    - 169.254.0.11:9092
  site_id: 1
```

In templates: `{{ site_vars.site_id }}`, `{% for s in site_vars.kafka_server %}{{ s }}{% endfor %}`.

## Bake

Embed config into a standalone binary:

```bash
dek bake ./dek -o mysetup
./mysetup              # show help with available configs
./mysetup apply        # apply all
./mysetup run deploy   # run commands
```
