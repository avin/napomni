# 04: Полное управление напоминаниями через CLI

**What to build:** Внешний агент управляет тем же списком напоминаний и с теми же возможностями, что внутренний чат: создаёт, изменяет, приостанавливает, возобновляет, удаляет и отменяет последнее изменение. Пользователь видит фактический результат в обычном интерфейсе, а расписания продолжают исполняться локально.

**Blocked by:** 02 — CLI: общий список и фоновый запуск приложения.

**Status:** resolved

- [x] CLI предоставляет создание, частичное изменение, паузу, возобновление, удаление и отмену последнего изменения в дополнение к общему списку из задачи 02.
- [x] Поддерживаются все текущие однократные, интервальные, недельные, месячные и годовые расписания, интервал после закрытия и существующие ограничения. Правила времени и проверки аргументов совпадают с внутренним чатом.
- [x] Все операции используют общее ядро и существующие прикладные правила; для них не вызывается внутренняя модель и не требуется подключённый провайдер.
- [x] Напоминание, созданное через чат, доступно внешнему списку и изменению по его стабильному идентификатору. Напоминание из CLI видно в интерфейсе и доступно внутреннему чату.
- [x] Результаты списка и команд содержат фактические данные, необходимые вызывающей стороне для выбора записи и проверки изменения. Совпадающий текст не используется как уникальный идентификатор разных напоминаний.
- [x] Частичное изменение сохраняет все неуказанные свойства и не затрагивает другие записи. Неверные аргументы и неизвестный идентификатор возвращают ошибку без изменения данных и последней отменяемой операции.
- [x] Отмена остаётся общей для приложения и действует по существующим правилам для создания, изменения, переключения паузы и удаления независимо от источника операции.
- [x] Изменение, пауза, удаление и отмена не меняют содержимое уже открытых тостов. Пауза и удаление сохраняют текущие правила отмены отложений; возобновление не переигрывает пропущенные срабатывания.
- [x] Успех изменяющей команды возвращается после сохранения фактического изменения. Ошибки сохранения не выдают ложного успеха и не повреждают существующие данные. Успешные изменения доступны после перезапуска.
- [x] Существующее восстановление операций внутреннего чата и его квитанции сохраняют поведение; эта задача не меняет правила повторных внешних разовых уведомлений.
- [x] Интеграционные проверки проходят через общий обработчик и публичное ядро с управляемым временем, временным хранилищем и отказами сохранения. Они охватывают работу с записью из чата, все операции, общую отмену, снимки открытых карточек и сохранение остальных свойств.
- [x] Настоящий CLI демонстрирует создание, получение результата из общего списка, изменение и удаление записи. Справка описывает реализованные команды и поддерживаемые формы расписаний.

## Answer

Implemented `create`, `update`, `pause`, `resume`, `delete`, and `undo` through the existing reminder tool dispatcher and shared Application. JSON is accepted as one argument or from UTF-8 stdin with `-`; update accepts `{id,patch}`, pause/resume/delete accept `{id}`, and list/undo take no arguments. Invalid objects/unknown fields are rejected without consuming shared undo. The captured external command context uses a constant source, never the JSON payload, so quoted reminder text remains literal.

`napomni-cli schema` prints exact argument schemas derived from the same tool definitions the chat receives, without launching the host. Actual `--help` documents all nine schedule forms, limits, partial updates, stable IDs, results, stdin, errors and uncertain responses. Notification command/help from ticket03 is preserved.

Evidence: failing public handler tests were observed for absent create/update/pause support, and failing real CLI tests for absent management/schema parsing before each implementation slice. Final serial verification after merging integration03: 72 tests passed and one private child-process fixture intentionally ignored across `external_management`, `reminder_cli`, `notify_cli`, `immediate_notifications`, `snooze`, `manage_reminders`, `mutation_tools`, `command_replay`, `calendar_dates`, `constraints`, `intervals`, `interval_after_close`.

Five new public handler tests cover chat interoperability and shared undo, exact quoted Unicode/newline text, partial edits and duplicate-text IDs, unknown IDs/invalid arguments, open snapshot preservation through pause/resume/delete/undo, durable restart, all schedule forms, and controlled save failure preserving committed state/undo. Four new real CLI tests use isolated copied binaries in paths with spaces; cover complete management, machine schemas, invalid inputs, filesystem save failure with no false success, and restart with saved undo. Windows restart tests wait for actual process termination to avoid racing old IPC teardown. Existing replay/receipt and scheduling regressions pass unchanged. Shared data/UI ownership comes from the existing desktop Application; no model/provider call is introduced.