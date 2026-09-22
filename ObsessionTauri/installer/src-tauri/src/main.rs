#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    if let Some(result) = obsession_setup_lib::machine_worker::dispatch_from_environment() {
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("obsession machine worker: {error}");
                ExitCode::FAILURE
            }
        };
    }

    // Наше окно — WebView2. Если рантайма в системе нет, красивый UI не
    // отрисуется вовсе: достаём payload и запускаем ВИДИМЫЙ стоковый NSIS —
    // он сам скачает и поставит WebView2 (webviewInstallMode по умолчанию).
    let arguments: Vec<_> = std::env::args_os().collect();
    if arguments.len() == 2
        && arguments.get(1).and_then(|value| value.to_str())
            == Some(obsession_setup_lib::UNINSTALL_SWITCH)
    {
        if obsession_setup_lib::webview2_present() {
            obsession_setup_lib::run();
            return ExitCode::SUCCESS;
        }
        return if obsession_setup_lib::run_uninstall() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }

    if !obsession_setup_lib::webview2_present() {
        obsession_setup_lib::run_fallback();
        return ExitCode::SUCCESS;
    }
    obsession_setup_lib::run();
    ExitCode::SUCCESS
}
