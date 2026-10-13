# Installation

You need Linux and a running Herdr session. Building from source requires Rust 1.88 or newer; running a prebuilt executable does not require Rust. The repository pins Rust 1.99.0 for its tested development toolchain. Herdr 0.9.3 is the initial integration baseline. Live observation also uses a compatible `herdr` command in `PATH`.

## Prebuilt packages

Each [release](https://github.com/SyscallBrain/lictor/releases/latest) ships three packages for Linux x86_64, with the same statically linked binary, the `lictor(1)` man page, and bash, zsh and fish completions. Check downloads against the release's `SHA256SUMS`:

```bash
sha256sum -c SHA256SUMS --ignore-missing
```

### Debian, Ubuntu and derivatives

```bash
sudo apt install ./lictor_*_amd64.deb
```

The package installs `/usr/bin/lictor`, the man page and the completions, which bash, zsh and fish pick up automatically. It suggests `libnotify-bin`, `tmux` and `xdotool` for the optional features. Remove it with `sudo apt remove lictor`.

### Arch Linux and derivatives

```bash
sudo pacman -U lictor-*-x86_64.pkg.tar.zst
```

The package installs the same files in Arch's standard locations, with the license in `/usr/share/licenses/lictor/`. Its optional dependencies are `libnotify`, `tmux` and `xdotool`. Remove it with `sudo pacman -R lictor`.

### Any other distribution

The binary in `lictor-VERSION-x86_64-unknown-linux-musl.tar.gz` is statically linked, so it runs on any x86_64 Linux: Arch, Fedora, openSUSE, Alpine, or an older glibc system. For your user only:

```bash
tar -xzf lictor-*-x86_64-unknown-linux-musl.tar.gz
cd lictor-*-x86_64-unknown-linux-musl
install -Dm755 lictor ~/.local/bin/lictor
install -Dm644 lictor.1.gz ~/.local/share/man/man1/lictor.1.gz
install -Dm644 completions/lictor.bash ~/.local/share/bash-completion/completions/lictor
install -Dm644 completions/lictor.fish ~/.config/fish/completions/lictor.fish
install -Dm644 completions/_lictor ~/.local/share/zsh/site-functions/_lictor
```

For zsh, make sure that directory is on your `fpath` before `compinit` runs, for example in `~/.zshrc`: `fpath=(~/.local/share/zsh/site-functions $fpath)`. For a system-wide install, use `/usr/local/bin`, `/usr/local/share/man/man1` and your distribution's completion directories with `sudo`.

## From source

Install the latest version from GitHub with Cargo:

```bash
cargo install --git https://github.com/SyscallBrain/lictor --locked
lictor --help
```

Or build from a clone:

```bash
git clone https://github.com/SyscallBrain/lictor
cd lictor
cargo install --path . --locked
```

`cargo build --release --locked` builds `target/release/lictor` without installing it. Check that Lictor can reach your Herdr session by listing its workspaces:

```bash
lictor -W -l
```

Check both the installed client and running server when diagnosing compatibility:

```bash
herdr --version
herdr status server
```

The core JSON socket client does not require `jq`, Python, or `socat`. Optional capabilities use external helpers:

| Helper | Purpose |
| --- | --- |
| `herdr` | Named-session discovery and read-only terminal observation. |
| `notify-send` | Desktop notifications after stopping. |
| `tmux` | Sidebar key delivery to a Herdr client inside tmux. |
| `xdotool` | Sidebar key delivery to an X11 terminal window. |

On a Wayland desktop, sidebar key delivery needs a supported route such as tmux; X11 injection alone cannot control a native Wayland window.

To read the book locally, install mdBook if it is missing, then build or serve it:

```bash
cargo install mdbook --version 0.5.4 --locked
mdbook build
mdbook serve --open
```

The generated book is written to `book/`. The repository includes workflows for testing, book publication, and Linux release artifacts; see [publishing](publishing.md).
