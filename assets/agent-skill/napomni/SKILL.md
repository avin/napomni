---
name: napomni
description: Show an immediate Windows notification or manage scheduled reminders with the user's local Napomni app.
---

# Napomni

Use the local CLI to show a message now or manage saved reminders. The desktop
application owns scheduling, storage, toast windows and sound; the CLI starts it
in the background when needed. No model, API key or internet connection is needed.

## Find the installed CLI

Read `cli-path.json` beside this file. Its `cli` property is the actual executable
path recorded by Napomni's settings. Resolve this resource against the directory
containing this skill, then use PowerShell's call operator for paths with spaces:

```powershell
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$cli = (Get-Content -LiteralPath (Join-Path $skillDirectory 'cli-path.json') -Raw -Encoding UTF8 | ConvertFrom-Json).cli
& $cli --help
```

Set `$skillDirectory` to the absolute directory containing this `SKILL.md`.
Keep both UTF-8 encoding settings in the invoking shell session: Windows
PowerShell 5 otherwise sends native stdin as ASCII and can replace Unicode text.
Use the executable's help as the command reference. For scheduled reminder
arguments and every supported schedule, read `& $cli schema` (JSON).

## Choose the operation

- **Notify now:** `& $cli notify $text`, passing the complete text as one argument.
  It creates an independent blue toast with sound and no saved reminder or
  schedule. Each call creates a separate card, including identical text. The
  result reports creation success or error; it gives no notification ID and
  does not wait for reading or closure. The user can close or snooze the card.
- **Schedule or manage reminders:** `list`, `create`, `update`, `pause`, `resume`,
  `delete`, and `undo` share the application's records and last undo operation,
  including changes from its internal chat. Select a record by its stable ID
  from `list`; duplicate text is allowed.

Management commands take one JSON object as an argument, or `-` to read UTF-8
JSON from stdin. `list` and `undo` take no arguments. For example:

```powershell
$request = @{text = 'Drink water'; schedule = @{kind = 'after_seconds'; seconds = 3600}}
$request | ConvertTo-Json -Depth 20 -Compress | & $cli create -
```

`update` takes `{id, patch}`; include only requested properties in `patch`.
`pause`, `resume`, and `delete` take `{id}`. Use the schema/help for property and
schedule details. List returns actual reminders with schedule descriptions and
statuses; mutations return saved facts or the deleted ID. Already open toast
snapshots keep their text, border and sound after reminder changes.

Check the exit code and returned JSON. Exit 0 confirms the application's result;
exit 1 supplies diagnostics on stderr. A lost reply can follow execution: report
the uncertainty and inspect reminder state where applicable. Do not automatically
repeat a notification after an uncertain response; a new call creates another card.
