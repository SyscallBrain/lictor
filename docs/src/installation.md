# Installation

You need Linux and a running Herdr session. Building from source requires Rust 1.88 or newer; running a prebuilt executable does not require Rust. The repository pins Rust 1.99.0 for its tested development toolchain. Herdr 0.9.3 is the initial integration baseline. Live observation also uses a compatible `herdr` command in `PATH`.

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
