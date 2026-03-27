## cc-connect Integration

This repo can be managed via `cc-connect`, enabling scheduled tasks and proactive notifications in chat.

### Scheduling

When a user asks to run something on a schedule (e.g. “every day at 9am”), run:

`cc-connect cron add "<schedule>" "<task command>" --force`

- `<schedule>` can be a cron expression or a natural-language schedule.
- `<task command>` should be the exact shell command to execute.
- Use `--force` only when updating an existing task.

### Proactive Messages

To send a message back to the current chat session:

`cc-connect send "<message>"`

### Session Environment

When `cc-connect` launches the agent, it sets:

- `CC_PROJECT`
- `CC_SESSION_KEY`
