# 02: CLI — общий список и фоновый запуск приложения

**What to build:** Внешний агент запускает отдельный консольный клиент и получает общий список напоминаний. Клиент подключается к работающему приложению либо запускает его в фоне, передаёт команду, получает результат и завершается. Основное приложение остаётся единственным владельцем данных и планировщика; установленная программа уже содержит оба исполняемых файла.

**Blocked by:** None (can start immediately).

**Status:** resolved

- [x] Поставляются основное GUI-приложение и отдельный консольный CLI-клиент. Обычный запуск, трей и открытие главного окна сохраняют привычное поведение.
- [x] Команда списка возвращает фактические сохранённые напоминания, стабильные идентификаторы, описания расписаний и текущие статусы, включая записи внутреннего чата.
- [x] Клиент использует локальный канал в контексте текущего пользователя Windows на том же компьютере. Для внешней команды не требуются провайдер, модель, API-ключ или интернет.
- [x] Если приложение работает, клиент передаёт команду ему. Если оно закрыто, клиент запускает его в фоне, дожидается готовности и получает результат без открытия или активации главного окна.
- [x] Одновременные холодные запуски клиентов приводят к одному владельцу состояния и одному планировщику. Повторный обычный запуск приложения также не создаёт независимого писателя в общее хранилище.
- [x] CLI не загружает отдельное изменяемое состояние напоминаний и не запускает собственный планировщик. Прикладной обработчик работает с тем же ядром, которым пользуются чат и интерфейс.
- [x] Успешное завершение клиента следует после результата команды. Ошибка запуска, недоступность приложения и неверная команда дают ошибочное завершение и доступную диагностику без ложного успеха и бесконечного ожидания.
- [x] Действительная справка CLI описывает доступную команду списка и её результат. Конкретные синтаксис и транспорт выбираются в рамках согласованной спецификации.
- [x] Существующая сборка и установщик включают оба исполняемых файла; установленный клиент находит установленное основное приложение, в том числе в каталоге с пробелами. Дополнительная галочка установки скилла не появляется.
- [x] Сквозные проверки запускают настоящий CLI с изолированными данными и обнаружением тестового экземпляра: работающий экземпляр, холодный запуск, несколько одновременных клиентов, неверная команда и ошибка запуска. Пользовательский экземпляр приложения не затрагивается.
- [ ] На Windows подтверждены фоновый запуск без активации главного окна и получение списка из установленной поставки. Поведение можно продемонстрировать независимо от команд изменения и разового показа.

## Answer

Implemented `napomni-cli list` and `--help`, a current-user local Windows named pipe with protected owner-only DACL and remote-client rejection, and OS file locking before desktop state is opened. Repeated and concurrent launches share the same Application and scheduler. Desktop dispatch acknowledges after native toast reconciliation; the client never opens reminder storage and never retries a sent command. Errors and application unavailability terminate with diagnostics and a bounded 20-second wait.

The installer includes both executables. Client discovery uses the sibling executable with native CreateProcessW and no inherited handles; paths containing spaces work. `NAPOMNI_DATA_DIR` isolates both application data and discovery; `NAPOMNI_APP_EXE` supports explicit host selection. README and actual CLI help document current behavior.

Evidence: `cargo test --test cli --test external_commands` passes (5 real CLI cases plus 1 Application case). Tests run copied real GUI/client binaries in isolated temporary directories with spaces; cover cold launch, reuse, six concurrent cold clients, repeat GUI launch, persisted chat data and exact Unicode/quotes/newlines, invalid input, failed process launch, and bounded unavailable-host failure. Native EnumWindows confirms the cold-launched host has no visible windows. Test owners are terminated; the installed user instance is untouched. Initial failing tests demonstrated the absent handler/CLI before implementation.

Manual gap: Inno Setup is not installed on this machine, so actual installer generation/install acceptance remains unverified. Copying the two binaries into a directory with spaces and observing hidden native windows is verified; this is not a claim that the installer was run, nor a direct foreground-activation observation. The final acceptance checkbox remains open for that manual check.