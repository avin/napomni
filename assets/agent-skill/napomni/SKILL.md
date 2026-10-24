---
name: napomni
description: Show an immediate Windows notification or manage scheduled reminders with the user's local Napomni app.
---

# Napomni

Use the local CLI for immediate notifications and saved reminders. It starts
the desktop app in the background when needed.

## Find the installed CLI

Set `$skillDirectory` to the absolute directory containing this `SKILL.md`.
Read the installed executable path from `cli-path.json` beside it; use
PowerShell's call operator for paths with spaces:

```powershell
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$cli = (Get-Content -LiteralPath (Join-Path $skillDirectory 'cli-path.json') -Raw -Encoding UTF8 | ConvertFrom-Json).cli
& $cli --help
```

Keep both UTF-8 encoding settings in the invoking shell session: Windows
PowerShell 5 otherwise sends native stdin as ASCII and can replace Unicode text.
If the resource or executable is missing, report that the skill needs to be
installed again through Napomni's settings. The resource is generated during
installation; the bundled source skill has no `cli-path.json`.

Use `--help` as the command reference.

## Notify now

Run `& (Join-Path $skillDirectory 'scripts/notify.ps1') -Text $text`.
The helper writes directly to console stdout/stderr; capture those streams
through the shell execution tool rather than assigning its output to a
PowerShell variable. Read `$LASTEXITCODE` immediately after the helper; when
running it as a standalone shell command, use `exit $LASTEXITCODE` to propagate
failure to the shell execution tool.

The helper sends exact UTF-8 text without an added newline, preserving quotes
and trailing backslashes that native argument marshalling can corrupt on
Windows PowerShell 5. Each call creates a separate toast, even for identical
text, with no saved reminder. Success confirms creation, not reading or closure;
there is no notification ID or status lookup.

## Schedule or manage reminders

Before a command that takes JSON, read `& $cli schema` for its argument shape,
supported rules and field semantics.

Use `list` to resolve an existing reminder's stable ID before `update`, `pause`,
`resume` or `delete`. If several records match, clarify using their text and
schedules. `undo` affects the shared last operation, which may come from the
app's chat rather than this agent's last command.

For a determined schedule, execute the requested operation. Ask only for missing
information needed to determine the schedule or identify the record. Preserve
quoted reminder text verbatim; otherwise use a short topic in the user's
language, or the localized equivalent of "Reminder" when no topic is supplied.
Leave unspecified sound and border settings to the app's defaults. Use relative
schedule kinds for delays and tomorrow so the app computes them at submission;
calendar times use Windows local time. Report unsupported rules rather than
approximating them.

For commands that take JSON, serialize an object and pass it through UTF-8 stdin
with `-` to avoid native argument quoting problems. For example:

```powershell
$request = @{text = 'Drink water'; schedule = @{kind = 'after_seconds'; seconds = 3600}}
$request | ConvertTo-Json -Depth 20 -Compress | & $cli create -
```

Include only requested properties in an update patch. `patch.limits` replaces
the whole limits object: merge requested changes with existing limits from
`list` to preserve unspecified constraints; use `{}` when asked to clear all
limits. Already open toasts keep their text, border and sound after reminder
changes.

## Verify the result

Check the exit code and returned JSON. Report the returned facts, including text
and schedule when present. Exit 0 confirms the application's result; exit 1
supplies diagnostics on stderr. A lost reply can follow execution: report
the uncertainty and inspect reminder state where applicable. Do not automatically
repeat a notification after an uncertain response; a new call creates another card.

A successful saved reminder operation may include `warnings` about desktop
display or sound. Report the saved facts and the warning separately. A notification
error saying creation could not be confirmed means a card may already exist;
apply the same uncertain-response rule.
