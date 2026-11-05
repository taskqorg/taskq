# TaskQ manager mode: stop

These steps are exact. Do them in order.

1. Delete the manager timer (PilotQ: `loop_stop`; Claude Code: delete its `/loop` job; any other agent: its own timer).
2. Read it back (PilotQ: `loop_status`). No manager timer may remain; otherwise repeat step 1.
3. Remove the standing instruction `Manager mode is on`. Tasks and questions stay as they are.
4. Reply exactly `Manager off`.

If a tick still arrives, do not act on it; reply `NO_CHANGE`.
