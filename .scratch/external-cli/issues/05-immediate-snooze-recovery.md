# 05: Отложение разовых уведомлений с восстановлением

**What to build:** Пользователь откладывает разовый тост обычной кнопкой карточки, временно прекращает его звук и получает один повторный показ. Отложение сохраняется после выхода из программы и восстанавливается в исходный срок либо сразу при запуске после срока.

**Blocked by:** 03 — Немедленный показ разового уведомления через CLI.

Status: resolved

- [x] Кнопка отложения разового тоста использует выбранную в настройках длительность, скрывает только выбранную карточку и прекращает её вклад в общий сигнал.
- [x] Отложение сохраняет снимок текста, рамки и звука и назначает один повторный показ. Напоминание или постоянное расписание при этом не создаются, последняя отменяемая операция управления напоминаниями не меняется.
- [x] Повторно показанную карточку можно снова отложить; каждое действие назначает один следующий показ без накопления дублей.
- [x] Несколько одинаковых по тексту разовых уведомлений откладываются и закрываются независимо. Обычные расписания не заменяют отложение разового уведомления.
- [x] После полного выхода и запуска до срока уведомление остаётся скрытым до первоначального времени. Новый отсчёт от запуска не начинается.
- [x] При запуске в срок или позже сохранённый тост показывается сразу. Успешно восстановленное отложение не создаёт повторных карточек при последующих опросах и перезапусках.
- [x] Уже открытая, не отложенная разовая карточка после полного выхода не восстанавливается. Программа различает сохранённое отложение и открытый тост.
- [x] При повторном показе восстанавливаются исходные содержимое и настройка звука. Общий сигнал учитывает остальные открытые звучащие карточки.
- [x] Ошибка сохранения отложения оставляет исходную карточку и её звук действующими. Сбой фиксации повторного показа не теряет сохранённое отложение и позволяет продолжить без дубля.
- [x] Расширенное состояние остаётся совместимым с ранее сохранёнными напоминаниями и обычными отложениями. Прежние правила расписаний, включая интервал после закрытия, не меняются.
- [x] Проверки управляемого времени и настоящего временного хранилища воспроизводят запуск до срока, в срок и после срока, повторное отложение, несколько карточек и ошибки записи без ожидания реальных минут.
- [ ] Сквозной сценарий начинается с карточки из настоящего CLI, продолжается отложением в интерфейсе и завершается повторным показом после перезапуска; на Windows проверяются фактическое скрытие карточки и остановка её звука.


## Answer

Independent notifications now use the existing snooze operation and configured duration without requiring a reminder. The snapshot and absolute deadline are persisted, future pending cards survive startup/resume, and due cards are consumed durably before one redisplay. A re-shown card can be snoozed again with the same identity and content. Open cards remain ephemeral. Reminder undo pruning now retains independent snoozes; ordinary reminder cancellation and interval-after-close behavior remain unchanged.

Red/green evidence: the public notify/snooze/restart scenario initially failed with `Toast has no reminder`; after implementation it preserves the original deadline and supports another snooze. The mixed-stack undo regression then failed because pending independent cards disappeared (one card instead of two at the deadline); retaining absent reminder associations fixed it.

Validation: `cargo test --test immediate_snooze --test snooze --test interval_after_close --test manage_reminders -- --nocapture` passed 42 tests, including 5 new controlled-clock scenarios using real temporary JSON stores. Coverage includes restart before/at/after the deadline, immutable snapshot identity/text/border/sound, duplicate-text cards, independent close/snooze and aggregate signal, unaffected regular schedules and undo, failed initial snooze save, repeated failed redisplay ticks, failed startup consumption, retry/restart recovery without loss or duplicates, legacy snapshots and after-close regressions. Save failures are injected at the public ReminderStore boundary around a real JsonStore. Touched Rust files formatted with rustfmt.

The CLI-to-native-button-to-restart acceptance scenario and human verification of actual sound cessation were not performed in this worktree; the remaining checklist item records that integration acceptance gap. The existing GPUI adapter passes the selected ToastId and configured snooze minutes through the operation covered here.
