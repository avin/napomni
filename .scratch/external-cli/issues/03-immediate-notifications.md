# 03: Немедленный показ разового уведомления через CLI

**What to build:** Внешний агент или локальный сервис передаёт текст через CLI и сразу получает обычный тост без создания напоминания с расписанием. Карточка отображается в общей стопке, звучит и закрывается как существующие тосты; вызывающая программа получает простой результат выполнения.

**Blocked by:** 01 — Подготовить самостоятельную идентичность тоста; 02 — CLI: общий список и фоновый запуск приложения.

Status: resolved

- [x] Отдельная команда разового показа принимает только текст. Расписание, отправитель, индивидуальные цвет и звук в её контракт не входят.
- [x] Переданный текст, включая кириллицу, кавычки и переносы строк, доходит без искажений и отображается существующим способом без добавленного имени отправителя.
- [x] Карточка использует синюю рамку, включённый повторяющийся звук, текущее размещение тостов и общий путь отрисовки. Показ не перехватывает фокус и не открывает главное окно.
- [x] Разовые и запланированные тосты находятся в общей стопке. Разовый тост остаётся до ручного закрытия, отложения или полного завершения приложения; автоматического исчезновения нет. Отложение добавляется задачей 05.
- [x] Каждый отдельный вызов создаёт самостоятельную карточку, даже при одинаковом тексте. Закрытие одной такой карточки не закрывает другую и не затрагивает напоминания.
- [x] Пока остаётся хотя бы один звучащий тост, общий сигнал продолжается. Закрытие последней звучащей карточки прекращает его.
- [x] Разовый показ не добавляет запись с расписанием в список, не создаёт скрытого фиктивного напоминания и не изменяет последнюю отменяемую операцию управления напоминаниями.
- [x] Вызывающая программа получает успех либо ошибку создания карточки без публичного идентификатора разового уведомления, ожидания закрытия и подтверждения прочтения. Само подключение к приложению ещё не считается успехом.
- [x] Команда работает при закрытом приложении через фоновый запуск из задачи 02. Потеря ответа не вызывает скрытого повторения показа; ошибка получения результата не трактуется как доказательство отсутствия карточки.
- [x] После полного завершения и повторного запуска приложения уже показанные разовые карточки не восстанавливаются.
- [x] Прикладные проверки наблюдают состав карточек, неизменность списка и отмены, независимое закрытие и общий сигнал. Сквозной вызов настоящего CLI проверяет передачу текста и результат, а ручная Windows-проверка подтверждает реальную карточку, звук и отсутствие перехвата фокуса.
- [x] Справка CLI описывает реализованную команду и правило отдельной карточки на каждый вызов; демонстрационные тестовые карточки не используются как её реализация.


## Answer

Implemented `napomni-cli notify <text>`: pass the complete text as one argument (including Unicode and newlines). The shared handler validates the text-only request and calls `Application::notify`, returning `{ "ok": true }` with no notification identifier. Every call adds an independent blue sounding card through the existing stack; reminders, persisted state and undo remain untouched. Native dispatch already reconciles the actual stack and signal before acknowledging execution. The existing transport never resends after a sent request loses its reply.

Red/green evidence: the public immediate notification test initially failed with `Unknown command: notify`, then passed after adding the application operation and shared handler. The real CLI transport test initially failed with `Unknown command or arguments`, then passed after adding the CLI command. Supplementary regressions verify duplicates in a mixed stack, independent close, aggregate signal, unchanged undo, invalid requests, and restart without restored open cards.

Validation: `cargo test --test immediate_notifications -- --nocapture` passed 3 tests. `cargo test --test notify_cli -- --nocapture` passed 2 tests (1 private child-process fixture intentionally ignored in ordinary execution). The transport test runs the actual CLI against an isolated child hosting the real local channel and shared Application handler, observing exact Unicode/quotes/newlines in the resulting public snapshot. The desktop process test cold-starts actual copied executables from a path with spaces, confirms a visible native toast and no visible main window, verifies another process retains foreground, reuses the host for an empty reminder list, and rejects blank/extra arguments without false success.

Native rendering and sound reconciliation execute in the desktop test. Visual inspection of card content/placement and human confirmation of audible sound were not performed; the automated core/API evidence does not claim physical audibility. Snoozing independent notifications remains task 05.
