# Agent status

Lictor uses Herdr's recognized-agent records rather than interpreting terminal text itself.

| Status | Meaning |
| --- | --- |
| `working` | The agent is busy. |
| `blocked` | Herdr recognized a request for attention, such as approval/input. |
| `done` | The agent is idle with a completion not yet marked seen. |
| `idle` | The agent is ready and its completion has been seen. |
| `unknown` | An agent is present but Herdr cannot classify its state confidently. |

Rotate only through agents needing attention:

```bash
lictor -A --status done,blocked -s 5
```

`--status` implies the agent group. The filter is checked on live state before each advance; agents can become eligible or ineligible after startup. If none match, rotation waits without exiting or spinning.

In a mixed queue, the filter affects agents only. Workspace and tab targets continue to be visited. In split observation, an ineligible agent lane waits while the other lane keeps cycling.

Explicit focus can mark a completion seen, changing `done` to `idle`. Read-only observation does not directly acknowledge the observed agent's completion. A server's status and a particular Herdr client's badge can differ because clients track viewed completions separately.

If an agent exits a selected pane, that agent target is removed. Lictor does not treat a later unrelated occupant of the same pane as the selected agent automatically. Unknown status is not evidence that a task completed successfully.
