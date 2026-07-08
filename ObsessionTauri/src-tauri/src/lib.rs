//! Obsession — DPI bypass launcher (Tauri backend).

mod admin;
mod autostart;
mod brain;
mod commands;
mod diag;
mod dpi;
mod eyes;
mod hosts;
mod lists;
mod net;
mod netcache;
mod netid;
mod paths;
mod profiles;
mod proxy;
mod ranking;
mod settings;
mod state;
mod util;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Listener, Manager, WindowEvent, Wry};

use state::AppState;

/// Пункты-галочки трея, отражающие состояние DPI/прокси. Храним, чтобы
/// синхронизировать их из слушателей событий статуса.
struct TrayMenu {
    dpi: CheckMenuItem<Wry>,
    proxy: CheckMenuItem<Wry>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // UAC-проверка ДО построения окна: winws и запись в hosts требуют прав
    // администратора. Если прав нет — пробуем перезапуститься с UAC.
    // В debug-сборке релонч отключён, чтобы `tauri dev` работал без раздвоения
    // процесса и с hot-reload (обход winws в dev без прав всё равно не сработает).
    #[cfg(all(windows, not(debug_assertions)))]
    if !admin::is_elevated() && admin::relaunch_as_admin() {
        // Пользователь принял UAC — elevated-инстанс запущен, выходим.
        std::process::exit(0);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let resource_dir = handle.path().resource_dir()?;
            let paths = paths::Paths::init(&resource_dir)?;
            let settings = settings::Settings::load(&paths.base_dir);
            // Старт свёрнутым: читаем ДО передачи settings во владение AppState.
            let start_minimized = settings.start_minimized;
            let auto_recovery = settings.auto_recovery;
            app.manage(AppState::new(paths, settings));

            // Подчищаем зависшие winws от предыдущего жёсткого выхода (иначе новый
            // инстанс падает «A copy of winws is already running»).
            #[cfg(windows)]
            {
                let orphans = dpi::detect_orphaned(&handle);
                if !orphans.is_empty() {
                    dpi::emergency_kill_all(&handle);
                }
            }

            // Если авто-восстановление включено в настройках — поднимаем Мозг сразу
            // (сессия откроется при следующем dpi_start).
            if auto_recovery {
                let bh = brain::runtime::start(handle.clone());
                *handle.state::<AppState>().brain.lock().unwrap() = Some(bh);
            }

            build_tray(app)?;

            // Синхронизация галочек трея с реальным состоянием DPI/прокси —
            // ловим те же события статуса, что и фронтенд.
            {
                let h = handle.clone();
                app.listen("dpi-status", move |event| {
                    let active = serde_json::from_str::<serde_json::Value>(event.payload())
                        .ok()
                        .and_then(|v| v.get("active").and_then(|b| b.as_bool()))
                        .unwrap_or(false);
                    if let Some(tm) = h.try_state::<TrayMenu>() {
                        let _ = tm.dpi.set_checked(active);
                    }
                });
                let h = handle.clone();
                app.listen("proxy-status", move |event| {
                    let running = serde_json::from_str::<serde_json::Value>(event.payload())
                        .ok()
                        .and_then(|v| v.get("running").and_then(|b| b.as_bool()))
                        .unwrap_or(false);
                    if let Some(tm) = h.try_state::<TrayMenu>() {
                        let _ = tm.proxy.set_checked(running);
                    }
                });
            }

            // Окно создано скрытым. Показываем, если не выбран старт в трее —
            // иначе приложение живёт в трее до клика по иконке.
            if !start_minimized {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle().clone();
                let minimize = app
                    .state::<AppState>()
                    .settings
                    .lock()
                    .unwrap()
                    .minimize_to_tray;
                if minimize {
                    // Сворачиваем в трей вместо выхода.
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    shutdown(&app);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::is_elevated,
            commands::diagnose,
            commands::get_autostart,
            commands::set_autostart,
            commands::dpi_start,
            commands::dpi_stop,
            commands::dpi_test,
            commands::dpi_detect_orphaned,
            commands::dpi_emergency_kill,
            commands::proxy_available,
            commands::proxy_start,
            commands::proxy_stop,
            commands::proxy_link,
            commands::hosts_status,
            commands::hosts_install,
            commands::hosts_uninstall,
            commands::get_settings,
            commands::save_settings,
            commands::lists_all,
            commands::read_list,
            commands::save_list,
            commands::create_list,
            commands::delete_list,
            commands::get_profiles,
            commands::save_profile,
            commands::delete_profile,
            commands::brain_set_enabled,
            commands::brain_get_status,
        ])
        .build(tauri::generate_context!())
        .expect("ошибка запуска Obsession")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                shutdown(app_handle);
            }
        });
}

/// Единый координатор завершения: гасит все дочерние процессы (winws + прокси).
/// Идемпотентен — безопасен при повторном вызове.
fn shutdown(app: &tauri::AppHandle) {
    // Синхронно убиваем свои процессы (в exit-хуке async-рантайм может не успеть).
    let (dpi_pids, proxy_pid) = {
        let state = app.state::<AppState>();
        let dpi_pids: Vec<u32> = state.dpi.lock().unwrap().procs.keys().copied().collect();
        let proxy_pid = state.proxy.lock().unwrap().pid;
        (dpi_pids, proxy_pid)
    };
    for pid in dpi_pids {
        let _ = util::std_command("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output();
    }
    if let Some(pid) = proxy_pid {
        let _ = util::std_command("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output();
    }
}

/// Строит иконку в системном трее с меню Показать/Выход.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let dpi = CheckMenuItem::with_id(app, "toggle_dpi", "DPI-обход", true, false, None::<&str>)?;
    let proxy =
        CheckMenuItem::with_id(app, "toggle_proxy", "Telegram-прокси", true, false, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let show = MenuItem::with_id(app, "show", "Показать", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&dpi, &proxy, &sep, &show, &quit])?;

    // Храним галочки для синхронизации из слушателей статуса.
    app.manage(TrayMenu {
        dpi: dpi.clone(),
        proxy: proxy.clone(),
    });

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Obsession")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle_dpi" => toggle_dpi(app),
            "toggle_proxy" => toggle_proxy(app),
            "show" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
            "quit" => {
                shutdown(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
        });

    // Иконка трея: используем распакованную tray.ico, иначе дефолтную оконную.
    let tray_path = app.state::<AppState>().paths.tray_icon_path();
    if let Ok(icon) = tauri::image::Image::from_path(&tray_path) {
        builder = builder.icon(icon);
    } else if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

/// Переключает DPI-обход из трея: конфиги берутся из сохранённых настроек
/// (с откатом к дефолтному конфигу категории, если явный выбор пуст).
fn toggle_dpi(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let active = {
            let st = app.state::<AppState>();
            let a = !st.dpi.lock().unwrap().procs.is_empty();
            a
        };
        if active {
            dpi::stop_all(&app).await;
        } else {
            let configs = {
                let st = app.state::<AppState>();
                let s = st.settings.lock().unwrap();
                let cats = if s.selected_categories.is_empty() {
                    vec!["discord".to_string()]
                } else {
                    s.selected_categories.clone()
                };
                cats.into_iter()
                    .map(|c| {
                        let file = s
                            .selected_configs
                            .get(&c)
                            .cloned()
                            .filter(|f| !f.is_empty())
                            .unwrap_or_else(|| default_config(&st.paths.get_configs_for_category(&c)));
                        (c, file)
                    })
                    .collect::<Vec<_>>()
            };
            let _ = dpi::start_many(&app, &configs).await;
        }
    });
}

/// Переключает Telegram-прокси из трея: порт/домен — из сохранённых настроек.
fn toggle_proxy(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let running = {
            let st = app.state::<AppState>();
            let r = st.proxy.lock().unwrap().pid.is_some();
            r
        };
        if running {
            proxy::stop(&app).await;
        } else {
            let (port, domain) = {
                let st = app.state::<AppState>();
                let s = st.settings.lock().unwrap();
                (s.proxy_port, s.fake_tls_domain.clone())
            };
            let _ = proxy::start(&app, port, &domain).await;
        }
    });
}

/// Дефолтный конфиг категории: `*_1.conf` или первый доступный.
fn default_config(files: &[String]) -> String {
    files
        .iter()
        .find(|f| f.contains("_1.conf"))
        .or_else(|| files.first())
        .cloned()
        .unwrap_or_default()
}
