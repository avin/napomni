# Installed skill forward test

PowerShell version: 5.1.19041.7802; environment NAPOMNI_DATA_DIR=C:/_/napomni-external-cli-check-1792784111199/forward-data; NAPOMNI_APP_EXE absent. Only installed skill/resources and executable help/schema consulted. Help and schema both exit 0. CLI: C:\_\napomni-external-cli-check-1792784111199\bin with spaces\napomni-cli.exe


## notify helper (Windows PowerShell 5)
Command/request: notify.ps1 -Text (exact UTF-8: Готово: "задача" + LF + Путь C:\work\; no final newline)
Exit: 0
Result:


## create
Command/request: {"text":"Выпить воды","schedule":{"kind":"after_seconds","seconds":3600}}
Exit: 0
Result: {"ok":true,"reminder":{"id":1,"text":"Выпить воды","schedule":{"kind":"once","at":"2026-10-24T09:27:58.885535900Z"},"border":5408245,"sound":true,"paused":false,"completed":false,"calendar_cursor":null},"schedule_description":"Однократно 24.10.2026 в 12:27:58 (местное время)","fact":"Сохранено: «Выпить воды». Однократно 24.10.2026 в 12:27:58 (местное время)","host_pid":7760}


## list
Command/request: (no arguments)
Exit: 0
Result: {"ok":true,"reminders":[{"reminder":{"id":1,"text":"Выпить воды","schedule":{"kind":"once","at":"2026-10-24T09:27:58.885535900Z"},"border":5408245,"sound":true,"paused":false,"completed":false,"calendar_cursor":null},"schedule_description":"Однократно 24.10.2026 в 12:27:58 (местное время)","schedule_status":"Следующее 24.10.2026 в 12:27:58"}],"host_pid":7760}

Notification helper wrote directly to Console.Out, bypassing same-process PowerShell output capture. Actual terminal output was {"ok":true,"host_pid":7760}, exit 0. It was invoked exactly once under PowerShell 5.1; no retry. The exact input was built with a literal quoted first line, LF, and literal trailing backslash.

## update
Command/request: {"id":1,"patch":{"text":"Сделать перерыв"}}
Exit: 0
Result: {"ok":true,"reminder":{"id":1,"text":"Сделать перерыв","schedule":{"kind":"once","at":"2026-10-24T09:27:58.885535900Z"},"border":5408245,"sound":true,"paused":false,"completed":false,"calendar_cursor":null},"schedule_description":"Однократно 24.10.2026 в 12:27:58 (местное время)","fact":"Изменено: «Сделать перерыв». Однократно 24.10.2026 в 12:27:58 (местное время). Звук: включён; не приостановлено","host_pid":7760}


## pause
Command/request: {"id":1}
Exit: 0
Result: {"ok":true,"reminder":{"id":1,"text":"Сделать перерыв","schedule":{"kind":"once","at":"2026-10-24T09:27:58.885535900Z"},"border":5408245,"sound":true,"paused":true,"completed":false,"calendar_cursor":null},"schedule_description":"Однократно 24.10.2026 в 12:27:58 (местное время)","fact":"Приостановлено: «Сделать перерыв». Однократно 24.10.2026 в 12:27:58 (местное время). Звук: включён; приостановлено","host_pid":7760}


## resume
Command/request: {"id":1}
Exit: 0
Result: {"ok":true,"reminder":{"id":1,"text":"Сделать перерыв","schedule":{"kind":"once","at":"2026-10-24T09:27:58.885535900Z"},"border":5408245,"sound":true,"paused":false,"completed":false,"calendar_cursor":null},"schedule_description":"Однократно 24.10.2026 в 12:27:58 (местное время)","fact":"Возобновлено: «Сделать перерыв». Однократно 24.10.2026 в 12:27:58 (местное время). Звук: включён; не приостановлено","host_pid":7760}


## undo
Command/request: (no arguments)
Exit: 0
Result: {"ok":true,"reminder":{"id":1,"text":"Сделать перерыв","schedule":{"kind":"once","at":"2026-10-24T09:27:58.885535900Z"},"border":5408245,"sound":true,"paused":true,"completed":false,"calendar_cursor":null},"schedule_description":"Однократно 24.10.2026 в 12:27:58 (местное время)","fact":"Отмена выполнена. Восстановлено: «Сделать перерыв». Однократно 24.10.2026 в 12:27:58 (местное время). Звук: включён; приостановлено","host_pid":7760}


## list
Command/request: (no arguments)
Exit: 0
Result: {"ok":true,"reminders":[{"reminder":{"id":1,"text":"Сделать перерыв","schedule":{"kind":"once","at":"2026-10-24T09:27:58.885535900Z"},"border":5408245,"sound":true,"paused":true,"completed":false,"calendar_cursor":null},"schedule_description":"Однократно 24.10.2026 в 12:27:58 (местное время)","schedule_status":null}],"host_pid":7760}


## delete
Command/request: {"id":1}
Exit: 0
Result: {"ok":true,"deleted_id":1,"fact":"Удалено: «Сделать перерыв»","host_pid":7760}


## list
Command/request: (no arguments)
Exit: 0
Result: {"ok":true,"reminders":[],"host_pid":7760}


Verified host PID 7760 executable Path C:\_\napomni-external-cli-check-1792784111199\bin with spaces\napomni.exe exactly matches isolated app before stopping.

Stopped isolated host PID 7760. Final list empty, all CLI exit codes 0. Installed instructions plus executable help/schema sufficed. Key pitfalls handled: paths with spaces need call operator; PS5 UTF-8 stdin settings; helper required for exact quotes/LF/trailing backslash; helper Console.Out is not captured by same-process assignment; undo after resume restores paused state. No repository code/spec/tests or agent notes consulted, and no installed skill/repository changes made.
