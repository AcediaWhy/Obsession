#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Наше окно — WebView2. Если рантайма в системе нет, красивый UI не
    // отрисуется вовсе: достаём payload и запускаем ВИДИМЫЙ стоковый NSIS —
    // он сам скачает и поставит WebView2 (webviewInstallMode по умолчанию).
    if !obsession_setup_lib::webview2_present() {
        obsession_setup_lib::run_fallback();
        return;
    }
    obsession_setup_lib::run();
}
