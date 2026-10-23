# External CLI — карта реализации

## Notes

Спецификация: [spec.md](spec.md). Интеграционная ветка: `codex/external-cli`.
Исходная точка кода: `ac86ce1`; спецификация зафиксирована в `4b9e0b0`.

Граф зависимостей: 01 и 02 независимы; 03 ← 01, 02; 04 ← 02; 05 ← 03; 06 ← 03, 04.
Проверяемые границы заданы разделом Testing Decisions спецификации.

## Decisions-so-far

- [05](issues/05-immediate-snooze-recovery.md): independent pending snapshots retain absolute deadlines and survive restart/undo; five new core scenarios plus existing snooze/after-close/mutation suites passed (42 tests). Native button/restart and physical sound acceptance remain to be checked.

- [03](issues/03-immediate-notifications.md): `notify <text>` creates an independent ephemeral card through the shared handler and native stack; exact CLI text transport and real native cold-launch visibility/focus verified. Physical audibility remains an integration check.

- [02: Local CLI host](issues/02-local-cli-host.md): Windows owner-only named pipe, pre-load file lock, JSON list, bounded result acknowledgement, sibling executable discovery; real isolated CLI tests pass. Installer execution/foreground manual acceptance remains open.

- [01](issues/01-independent-toast-identity.md): independent UUID card identity, optional reminder association and observed-card actions; legacy snooze compatibility and existing scheduling behavior covered by public core tests.

## Fog

- Визуальные и звуковые результаты Windows отмечаются отдельно от проверок ядра и процессов.
