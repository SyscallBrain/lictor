# Target selectors

List targets before using numeric selectors:

```bash
lictor -l
lictor -T -l
lictor -W -l
lictor -A -l
```

Indices start at one. Names and identifiers are case-sensitive. Herdr IDs are opaque; use the IDs returned by listing instead of predicting their counters.

| Mode | Selector | Meaning |
| --- | --- | --- |
| Pane | `N` | Pane N in the baseline tab. |
| Pane | `T.P` | Pane P in tab T of the baseline workspace. |
| Pane | `W:T.P` | Pane P in tab T of workspace W. |
| Tab | `N` | Tab N of the baseline workspace. |
| Tab | `W:N` | Tab N of workspace W. |
| Workspace | `N` or a name | Workspace number or label. |
| Agent | `N`, a name, or a kind | Agent ordinal, unique agent name, or every initial agent of that kind. |
| Explicit ID | `w1:p2`, `w1:t2`, `w1` | A pane, tab, or workspace handle from Herdr. |

In agent mode, a pane ID must contain a recognized agent. A kind such as `claude` expands to matching agents; it does not create an agent.

```bash
lictor -p '2:1.1,3:1.2' -s 5
lictor -T -p 'api:2,web:1' -s 5
lictor -A -p 'claude,reviewer' -s 5
```

Untyped mixed workspace/agent selectors preserve the original precedence: a matching workspace wins before an agent. For mixed selections, prefer explicit typed selectors:

```bash
lictor -T -W -A --targets 'workspace=api,tab=w2:t3,agent=reviewer'
```

The typed prefixes are `workspace=`, `tab=`, `agent=`, and `pane=`. This avoids confusion between a workspace called `claude` and the `claude` agent kind.

Split lanes have separate selectors:

```bash
lictor -S w,a --left-targets 'api,web' --right-targets 'reviewer,codex'
lictor -S t,a --left-targets 'w1:t1,w2:t2' --right-targets 'reviewer'
```

Use lane flags instead of `-p` in split mode. Each explicit ID must match its lane kind. Repeated entries remain repeated; any nonexistent requested target rejects the whole invocation before view creation.
