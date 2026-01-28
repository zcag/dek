# dek

*pronounced "deynek"*

Declarative environment setup. One TOML, any machine.

## Install

```bash
cargo install dek
# or
curl cagdas.io/dek | sh

# setup completions
dek setup
```

## Usage

```bash
dek apply              # apply ./dek.toml or ./dek/
dek apply setup.toml
dek check              # dry-run, show what would change
dek plan               # list items from config (no state check)
```

## Config

```toml
# Packages
[package.os]  # auto-detects: pacman, apt, dnf, brew
items = ["curl", "git", "htop"]

[package.cargo]  # auto-installs cargo/go/npm/pip if missing
items = ["bat", "eza", "ripgrep"]

[package.go]
items = ["github.com/junegunn/fzf@latest"]

[package.npm]
items = ["prettier", "typescript"]

[package.pip]
items = ["httpie", "tldr"]

# Systemd services
[[service]]
name = "docker"
state = "active"
enabled = true

# Files
[file.copy]
"dotfiles/.zshrc" = "~/.zshrc"

[file.symlink]
"~/dotfiles/nvim" = "~/.config/nvim"

[file.ensure_line]
"~/.bashrc" = [
    "export PATH=$HOME/.local/bin:$PATH",
    "source ~/.aliases"
]

# Shell (auto-sources in your rc file)
[alias]
la = "ls -larth"
lg = "lazygit"

[env]
EDITOR = "nvim"
```

## Split Config

```
dek/
├── 00-packages.toml
├── 10-services.toml
├── 20-dotfiles.toml
└── 30-shell.toml
```

Files are merged alphabetically.

## Test

Spin up a container, apply config, drop into shell:

```bash
dek test                    # archlinux by default
dek test --image ubuntu
dek test --keep             # don't destroy after exit
```

Builds dek locally and mounts into container - no compilation inside the container.

## Inline

Quick one-off installs without a config file:

```bash
dek os.htop os.git cargo.bat
dek pip.httpie npm.prettier
```

Format: `provider.package` where provider is `os`, `apt`, `pacman`, `cargo`, `go`, `npm`, or `pip`.

`os` auto-detects your system package manager (pacman, apt, dnf, brew).

## Remote (planned)

```bash
dek --target user@host apply
dek --target user@host cargo.bat
```

## Bake (planned)

Embed config into a standalone binary:

```bash
dek bake dek.toml -o mysetup      # from file
dek bake dek/ -o mysetup          # from directory
./mysetup apply                    # runs anywhere, no deps
```

## TODO

- Package name → binary name mapping: currently hardcoded (e.g. `ripgrep` → `rg`). Consider allowing config to specify binary name:
  ```toml
  [package.cargo]
  items = [
    "bat",
    { pkg = "ripgrep", bin = "rg" },
    { pkg = "fd-find", bin = "fd" },
  ]
  ```
