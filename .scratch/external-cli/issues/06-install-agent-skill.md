# 06: Установка английского скилла из настроек

**What to build:** Пользователь открывает настройки, видит подставленную персональную папку скиллов и устанавливает один комплектный английский скилл кнопкой. Он может заменить путь и повторить установку в другое место. Агент по установленной инструкции использует фактический CLI для немедленного показа и полного управления напоминаниями.

**Blocked by:** 03 — Немедленный показ разового уведомления через CLI; 04 — Полное управление напоминаниями через CLI.

**Status:** resolved

- [x] В настройках доступны редактируемое поле «Папка скиллов» и кнопка «Установить скилл». Исходный путь — стандартный персональный каталог скиллов текущего пользователя, зафиксированный спецификацией.
- [x] По кнопке один комплектный скилл устанавливается в собственную подпапку выбранного корня с корректным SKILL.md. Его инструкции написаны на английском языке.
- [x] Скилл охватывает оба согласованных сценария: немедленный показ разового уведомления только по тексту и все реализованные операции управления напоминаниями. Инструкции и примеры соответствуют действительным справке, аргументам и результатам CLI.
- [x] Инструкция объясняет различие разового уведомления и напоминания с расписанием, простой результат разового показа и отдельную карточку на каждый новый вызов. Она не обещает прочтение сообщения или автоматическую дедупликацию.
- [x] Установленный скилл или его комплектные ресурсы позволяют найти фактический CLI, включая установку программы в нестандартную папку с пробелами. Инструкция не привязана к каталогу разработчика.
- [x] Пользователь может изменить путь и повторно нажать кнопку для установки в другое место. Прежняя копия и посторонние скиллы остаются на месте; установка не выполняет перенос или удаление старой копии.
- [x] Успех отображается после установки необходимых файлов. Ошибки пути, создания каталога или записи доступны пользователю и не отображаются как успешная установка.
- [x] Установка выполняется локально без интернета, скачивания сторонних компонентов, модели и API-ключа.
- [x] Ресурсы скилла входят в обычную поставку вместе с GUI-приложением и CLI. Новая галочка установщика и автоматическая установка скилла при первом запуске не добавляются.
- [x] Проверки вызывают публичное действие установки с временными целевыми папками и наблюдают реальные файлы, выбранный корень, фактический путь CLI, сохранение предыдущей копии и соседних скиллов, а также ошибки записи. Они не закрепляют внутреннее дерево GPUI.
- [x] Скилл проходит проверку формата и практический изолированный сценарий: агент по установленной английской инструкции показывает разовое уведомление и управляет напоминанием через реализованный CLI.
- [ ] В установленной Windows-программе вручную проверены поле пути, нажатие кнопки, результат успешной установки и видимая ошибка неподходящего назначения.
- [x] Задача не требует завершения отложений из задачи 05: установленный скилл описывает внешний CLI, а отложение выполняется пользовательской кнопкой карточки.

## Answer

Implemented a dedicated integration card directly in the main Settings page (`MainWindow::connection_settings`), with editable «Папка скиллов» initialized to current USERPROFILE/.agents/skills and «Установить скилл». The click invokes public `skill_install::install(root, cli)` and displays success only after all required files are written; path/create/write errors appear in the existing Settings status area. Changing the field and installing again leaves previous roots untouched. The field stays in the current window; persistence across application restarts was not required by the spec.

The offline bundled English skill installs under the selected root's own `napomni` folder. Generated UTF-8 `cli-path.json` records the actual absolute sibling CLI path using JSON serialization, so nonstandard paths/spaces/Unicode are handled. The skill loads this resource relative to its own location, uses the real CLI help/schema for current commands, distinguishes immediate cards and saved reminders, covers management and shared undo, and explains separate cards for repeated text, simple creation acceptance, no read/closure guarantee, and uncertain reply handling. PowerShell examples explicitly set UTF-8 native stdin/output encoding and read the path resource as UTF-8, including Windows PowerShell5. Installer ships the bundled SKILL.md with both binaries; no installer task/first-run installation was added.

Evidence: initial filesystem boundary test failed because skill_install was absent before implementation. All three public installation tests pass: real files and current-user default, invoking the real copied CLI via the generated resource in a path with spaces, choosing another root while preserving previous/neighboring skills, reinstallation, invalid/empty roots, missing CLI, and required-file write failures. UI binary compiles as part of cargo test. Format validator passes: `python -X utf8 C:/Users/avin/.codex/skills/.system/skill-creator/scripts/quick_validate.py assets/agent-skill/napomni`.

Remaining integration acceptance explicitly pending: independent forward test through the installed English instruction, and Windows UI/installed-package checks of field/button/success/error. Root implementation agent will perform these after merge. Source format validation and copied executable/schema checks do not claim that those acceptance scenarios were completed. Latest integration including ticket05 has been merged before handoff.

Final integration acceptance (2026-10-24): the publicly installed copy passed format validation and an independent agent completed notify/create/list/update/pause/resume/undo/delete using only the English skill and executable help/schema under Windows PowerShell 5.1. See docs/verification/external-cli.md and external-cli-skill-forward.md. Native Settings interaction remains unverified because computer-use capture timed out; this does not claim button-level acceptance. Review fixes also ship scripts/notify.ps1 as a required resource for exact UTF-8 stdin notification text.
