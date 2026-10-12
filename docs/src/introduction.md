# Introduction

Lictor rotates through the work running in [Herdr](https://herdr.dev/): terminal panes, tabs, workspaces, and recognized coding agents. Choose targets from the command line or from a keyboard-driven Ratatui interface.

![A Lictor mirror tab: a workspace with its agent and server log on the left, a blocked agent on the right](images/mirror.png)

When several coding agents work in parallel, the hard part is noticing which one has finished, is stuck, or is waiting for you. Lictor turns Herdr into a dashboard you can glance at.

There are two ways to watch your work:

- **Focus rotation** switches Herdr's focused target every few seconds, like the original `herdr_rotate` script.
- **Split observation** opens a new Herdr tab containing two viewers. Each viewer cycles through its own selected targets while the original terminals continue in their existing locations.

A workspace or tab viewer reconstructs the entire source tab's pane arrangement. An agent viewer shows the terminal hosting that agent. The viewers observe existing terminal output; they do not launch another copy of an agent.

Both modes are safe by design: Lictor never sends input to your agents, never moves or resizes panes, and only closes tabs it created.

Lictor starts on Linux. Its TUI uses Vim-style navigation and the Tokyo Night palette. The CLI and this book are in English.

Start with [installation](installation.md) and [quick start](quick-start.md). Read [split observation](split-observation.md) for side-by-side monitoring, [target selectors](selectors.md) to build a precise rotation queue, or [how Lictor works](how-it-works.md) for what happens behind the scenes.

The project is a Rust port and extension of Tiago Correia's [`herdr_rotate` gist](https://gist.github.com/SyscallBrain/e6534601ece6d956e922c589f517078c). Lictor is licensed under the MIT License, copyright 2026 Tiago Correia. Herdr is a separate project with its own license.
