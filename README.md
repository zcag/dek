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
```

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
check = "docker --version"
stdout = "Docker version 2[0-9]"

[[assert]]
check = "test -f /etc/hosts"
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
├── meta.toml           # name, description, banner
├── 00-packages.toml
├── 10-services.toml
├── 20-dotfiles.toml
└── optional/
    └── extra.toml      # only applied when explicitly requested
```

Files merged alphabetically. Use `dek apply extra` to include optional configs.

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
```

```bash
dek run              # list available commands
dek run deploy       # run command
dek run backup arg1  # args passed via $@
```

## Remote

Apply to remote hosts via SSH:

```bash
dek apply -t user@host
dek check -t server1
```

Use `-q`/`--quiet` to suppress banners (auto-enabled for multi-host).

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

For build-and-deploy workflows:

```toml
# Local build step (runs before remote)
[run.build]
local = true
cmd = "mvn package -DskipTests"

# Include build artifacts
[include]
"target/app.jar" = "artifacts/app.jar"

# Deploy to remote
[file.copy]
"artifacts/app.jar" = "/opt/app/app.jar"

[[service]]
name = "app"
state = "active"
```

```bash
dek apply -r 'app-*'
# 1. Runs build locally
# 2. Includes fresh jar
# 3. Ships to all app-* hosts in parallel
# 4. Copies jar, restarts service
```

## Inline

Quick installs without a config file:

```bash
dek os.htop os.git cargo.bat
dek pip.httpie npm.prettier
```

## Test

Spin up a container to test your config:

```bash
dek test                    # archlinux by default
dek test --image ubuntu
dek test --keep             # keep container after exit
```

## Bake

Embed config into a standalone binary:

```bash
dek bake ./dek -o mysetup
./mysetup              # show help with available configs
./mysetup apply        # apply all
./mysetup run deploy   # run commands
```
