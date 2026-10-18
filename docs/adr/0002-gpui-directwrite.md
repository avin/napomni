# Нативный интерфейс на Rust через GPUI и DirectWrite

Используем GPUI вместо egui/eframe: пользователь поставил обязательным требованием визуально чёткий текст и предпочёл сохранить полностью Rust-интерфейс без веб-технологий. Windows-бэкенд GPUI использует DirectWrite для текста и предоставляет всплывающие окна поверх остальных с первоначальным показом без активации; оформление и поведение стопки в стиле Base UI реализуем самостоятельно.

Принимаем зависимость от pre-1.0 API GPUI и фиксируем совместимую версию. Чёткость кириллицы на разных масштабах Windows, прозрачность и отсутствие захвата фокуса проверяем при реализации. Tauri остаётся запасным вариантом, если нативный подход не удовлетворит требованиям; переход на него этим решением не утверждён.

Источники: [GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md), [DirectWrite](https://github.com/zed-industries/zed/blob/main/crates/gpui_windows/src/direct_write.rs), [Windows windows](https://github.com/zed-industries/zed/blob/main/crates/gpui_windows/src/window.rs).
