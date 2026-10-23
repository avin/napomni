# 01: Подготовить самостоятельную идентичность тоста

**What to build:** Подготовить существующие карточки к разовым уведомлениям без расписания: тост имеет собственную идентичность и может быть связан с напоминанием. При этом пользователь продолжает получать, закрывать и откладывать обычные напоминания по прежним правилам. Это предварительный рефакторинг; внешний разовый показ добавляется отдельной задачей.

**Blocked by:** None (can start immediately).

Status: resolved

- [x] У тоста есть самостоятельная идентичность и необязательная связь с напоминанием; идентичности карточки и напоминания не смешиваются.
- [x] Связь с напоминанием сохраняет правило одного одновременно открытого тоста на напоминание. Очередное срабатывание не меняет содержимое и порядок уже открытой карточки.
- [x] Через существующие пользовательские действия закрывается или откладывается именно выбранный тост; остальные карточки и их звук сохраняются.
- [x] Снимок текста, рамки и звука остаётся неизменным при изменении, паузе, удалении или отмене изменения исходного напоминания.
- [x] Общий сигнал согласуется с открытыми звучащими карточками и прекращается после закрытия или отложения последней такой карточки.
- [x] Читаются ранее сохранённые напоминания и отложения. Перезапуск, пропущенные срабатывания и режим интервала после закрытия сохраняют существующее поведение.
- [x] Не добавляются фиктивные напоминания, пользовательские команды разового показа или новые настройки: задача подготавливает модель и адаптер существующих карточек.
- [x] Проверки проходят через публичные операции ядра с управляемым временем, наблюдаемые карточки и реальное временное хранилище. Сохраняются проверки стопки, снимков, общего звука, отложений и восстановления старых данных.
- [x] Результат проверяем самостоятельно: обычное напоминание создаётся, срабатывает, закрывается и откладывается после рефакторинга, а существующие интеграционные проверки остаются успешными.


## Answer

Implemented independent `ToastId` (UUID), optional reminder association, and card-addressed close/snooze actions throughout the core and GPUI stack. A new scheduled occurrence gets its own identity; snoozing preserves the card identity and immutable content while advancing display sequence. Legacy saved snapshots without an ID receive one during deserialization.

Red/green evidence: the public stale-card action regression initially failed to compile without the new identity/association; it now passes. A real legacy JSON fixture failed with `missing field id` before adding the compatibility default; the focused test now passes through restart, deadline, repeated snooze and close.

Validation: `cargo test --tests --no-fail-fast` passed every preexisting test and the stale-card test; its only failure was the new legacy fixture before restoring its compatibility default. After the fix, `cargo test --test snooze legacy_snoozed -- --nocapture` passed. The suites cover stack ordering, immutable snapshots, aggregate signal, persisted snoozes, missed occurrences, calendar schedules, and interval-after-close. GPUI binary compiled as part of the test command. Touched Rust files formatted with rustfmt.

Manual Windows visual/audio verification was not performed: these automated results establish core behavior and adapter compilation, not visible windows, actual focus behavior or audible sound. Those checks remain part of integration acceptance.
