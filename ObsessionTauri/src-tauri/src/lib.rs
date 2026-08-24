//! Obsession — DPI bypass launcher (Tauri backend).

mod adaptive_strategy;
mod admin;
mod ai_probe;
mod autostart;
mod brain;
mod commands;
mod diag;
mod dpi;
mod dpi_engine;
mod dpi_supervisor;
mod eyes;
mod hosts;
mod hosts_snapshot;
mod hosts_validate;
mod legacy_reliability;
mod lists;
mod lists_validate;
mod net;
mod netcache;
mod netid;
mod onboarding;
mod paths;
mod profiles;
mod protected_runtime;
mod proxy;
mod ranking;
mod security;
mod settings;
mod state;
mod util;
mod webmem;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Listener, Manager, WindowEvent, Wry};

use state::AppState;
use util::LockExt;

/// Идёт ли уже завершение приложения. Гейтит `begin_exit`, чтобы крестик/трей и
/// последующий `RunEvent::ExitRequested` не запустили teardown дважды.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Пункты-галочки трея, отражающие состояние DPI/прокси. Храним, чтобы
/// синхронизировать их из слушателей событий статуса.
struct TrayMenu {
    dpi: CheckMenuItem<Wry>,
    proxy: CheckMenuItem<Wry>,
}

/// Хэндл трей-иконки + текущее состояние DPI/прокси. Нужен, чтобы менять иконку
/// (активная/пассивная) и тултип из слушателей статуса.
struct TrayState {
    tray: TrayIcon<Wry>,
    idle_path: std::path::PathBuf,
    active_path: std::path::PathBuf,
    dpi: AtomicBool,
    proxy: AtomicBool,
}

/// Обновляет иконку и тултип трея под текущее состояние. `dpi`/`proxy` — новые
/// значения (если известны); иначе берём сохранённые.
fn refresh_tray(app: &tauri::AppHandle, dpi: Option<bool>, proxy: Option<bool>) {
    let Some(ts) = app.try_state::<TrayState>() else {
        return;
    };
    if let Some(d) = dpi {
        ts.dpi.store(d, Ordering::SeqCst);
    }
    if let Some(p) = proxy {
        ts.proxy.store(p, Ordering::SeqCst);
    }
    let dpi_on = ts.dpi.load(Ordering::SeqCst);
    let proxy_on = ts.proxy.load(Ordering::SeqCst);
    let path = if dpi_on || proxy_on {
        &ts.active_path
    } else {
        &ts.idle_path
    };
    if let Ok(img) = tauri::image::Image::from_path(path) {
        let _ = ts.tray.set_icon(Some(img));
    }
    let tip = format!(
        "Obsession · DPI: {} · Прокси: {}",
        if dpi_on { "вкл" } else { "выкл" },
        if proxy_on { "вкл" } else { "выкл" },
    );
    let _ = ts.tray.set_tooltip(Some(tip));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Single-instance ПЕРВЫМ плагином (требование tauri-plugin-single-instance):
        // второй запуск лаунчера не плодит процесс/трей-иконку, а поднимает уже
        // открытое окно первого инстанса. UI всегда остаётся обычным процессом:
        // UAC-релонч удалён, пока нет защищённого helper/service.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
                let _ = app.emit("window-visibility", true);
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // Глобальный хоткей: единственный обработчик — на нажатие переключаем
        // защиту (та же логика, что у тумблера в трее). Саму комбинацию
        // регистрируем в setup (там доступен AppHandle и можно мягко пережить,
        // если сочетание занято другим приложением).
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        toggle_dpi(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let resource_dir = handle.path().resource_dir()?;
            let paths = paths::Paths::init(&resource_dir)?;
            let settings = settings::Settings::load(&paths.base_dir);
            // Старт свёрнутым: читаем ДО передачи settings во владение AppState.
            let start_minimized = settings.start_minimized;
            let auto_recovery = settings.auto_recovery;
            let hotkey_toggle = settings.hotkey_toggle.clone();
            app.manage(AppState::new(paths, settings));

            // Adaptive Zapret2 coordinator изолирован от Legacy Brain и всегда
            // готов принять passive observations/явную команду поиска. До
            // нажатия пользователя он не меняет DPI runtime.
            let adaptive = adaptive_strategy::runtime::start(handle.clone());
            *handle.state::<AppState>().adaptive.lock_recover() = Some(adaptive);

            // Подчищаем зависшие winws ОТ ПРЕДЫДУЩЕГО жёсткого выхода (иначе новый
            // инстанс падает «A copy of winws is already running»). Бьём точечно по
            // обнаруженным PID, а не глобально по имени образа — иначе снесли бы
            // ЧУЖОЙ winws.exe (параллельный Zapret/GoodbyeDPI пользователя).
            #[cfg(windows)]
            {
                if security::protected_runtime_available() {
                    let orphans = dpi::detect_orphaned(&handle);
                    dpi::kill_orphans(&handle, &orphans);
                } else {
                    util::emit_log(
                        &handle,
                        "warn",
                        "security",
                        "Привилегированный runtime временно отключён: приложение запущено без UAC и не исполняет компоненты из AppData.",
                    );
                }
            }

            // Если авто-восстановление включено в настройках — поднимаем Мозг сразу
            // (сессия откроется при следующем dpi_start).
            if auto_recovery && security::protected_runtime_available() {
                let bh = brain::runtime::start(handle.clone());
                *handle.state::<AppState>().brain.lock_recover() = Some(bh);
            }

            build_tray(app)?;

            // Глобальный хоткей (по умолчанию Ctrl+Shift+O) — вкл/выкл защиты из
            // любого места, в т.ч. из свёрнутого в трей окна. Сочетание берём из
            // настроек (меняется командой set_hotkey). Пустое = выключен; занятость
            // сочетания другим приложением не фатальна — логируем и продолжаем.
            #[cfg(desktop)]
            if security::protected_runtime_available() && !hotkey_toggle.trim().is_empty() {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                if let Err(e) = handle.global_shortcut().register(hotkey_toggle.as_str()) {
                    util::emit_log(
                        &handle,
                        "warn",
                        "Хоткей",
                        &format!("Не удалось включить «{hotkey_toggle}»: {e}"),
                    );
                }
            }

            // Синхронизация галочек трея с реальным состоянием DPI/прокси —
            // ловим те же события статуса, что и фронтенд.
            {
                let h = handle.clone();
                app.listen("dpi-status", move |event| {
                    let active = serde_json::from_str::<serde_json::Value>(event.payload())
                        .ok()
                        .and_then(|v| v.get("value")?.get("active").and_then(|b| b.as_bool()))
                        .unwrap_or(false);
                    if let Some(tm) = h.try_state::<TrayMenu>() {
                        let _ = tm.dpi.set_checked(active);
                    }
                    refresh_tray(&h, Some(active), None);
                });
                let h = handle.clone();
                app.listen("proxy-status", move |event| {
                    let running = serde_json::from_str::<serde_json::Value>(event.payload())
                        .ok()
                        .and_then(|v| v.get("value")?.get("running").and_then(|b| b.as_bool()))
                        .unwrap_or(false);
                    if let Some(tm) = h.try_state::<TrayMenu>() {
                        let _ = tm.proxy.set_checked(running);
                    }
                    refresh_tray(&h, None, Some(running));
                });
            }

            // Надёжный детектор видимости окна для паузы анимаций. Событийный путь
            // в связке tao+WebView2 капризен: сворачивание/разворот не всегда шлют
            // Resized, а window.hide() не даёт visibilitychange. Причём tao-обёртки
            // is_minimized()/is_visible() под WebView2 ВРУТ (лагают/не видят внешний
            // минимайз) — из-за этого сворачивание не паузило анимации и окно жгло
            // CPU в трее. Берём состояние напрямую из Win32 (IsIconic/IsWindowVisible)
            // — авторитетный источник — и шлём window-visibility при изменении.
            // Интервал 300мс: пауза срабатывает почти мгновенно, нагрузка околонулевая.
            {
                let h = handle.clone();
                std::thread::spawn(move || {
                    let mut last_shown: Option<bool> = None;
                    // Обратный отсчёт до трима рабочего набора после скрытия в трей
                    // (в тиках по 300мс). 0 = трим не запланирован.
                    let mut trim_after: u32 = 0;
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        // При выходе прекращаем поллинг: иначе поток вечно дёргает
                        // webmem/emit на умирающем AppHandle. try_state, не state():
                        // в конце teardown Tauri может снять state до завершения
                        // потока — state() там паникует (узкое, но реальное окно).
                        let Some(state) = h.try_state::<AppState>() else {
                            break;
                        };
                        if state.shutting_down.load(Ordering::SeqCst) {
                            break;
                        }
                        let Some(win) = h.get_webview_window("main") else {
                            continue;
                        };
                        #[cfg(windows)]
                        let shown = match win.hwnd() {
                            Ok(h) => unsafe {
                                use windows::Win32::Foundation::HWND;
                                use windows::Win32::UI::WindowsAndMessaging::{
                                    IsIconic, IsWindowVisible,
                                };
                                // Реконструируем HWND нашей версии windows-крейта из
                                // сырого указателя: у tauri своя версия крейта, прямая
                                // передача её HWND не типизируется.
                                let hwnd = HWND(h.0 as _);
                                IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool()
                            },
                            Err(_) => continue,
                        };
                        #[cfg(not(windows))]
                        let shown = win.is_visible().unwrap_or(true)
                            && !win.is_minimized().unwrap_or(false);
                        if last_shown != Some(shown) {
                            last_shown = Some(shown);
                            let _ = h.emit("window-visibility", shown);
                            // Экономия RAM в трее: LOW при скрытии, NORMAL на показе.
                            // Фронт уже выставил IsVisible=false (getCurrentWebview
                            // .hide()) — повторно его НЕ трогаем (тек. GDI, см. webmem).
                            webmem::set_low_memory(&h, !shown);
                            // Трим рабочего набора отложенно: даём Chromium осесть
                            // после LOW, и только если окно всё ещё скрыто.
                            trim_after = if shown { 0 } else { 5 };
                        } else if trim_after > 0 && !shown {
                            trim_after -= 1;
                            if trim_after == 0 {
                                webmem::trim_working_set();
                            }
                        }
                    }
                });
            }

            // Окно создано скрытым. В dev/debug показываем всегда, в release — если не выбран старт в трее.
            if !start_minimized || cfg!(debug_assertions) {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.unminimize();
                    let _ = win.set_focus();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle().clone();
                // lock_recover, НЕ lock().unwrap(): этот обработчик крутится на
                // главном потоке event-loop. Отравленный паникой другого держателя
                // settings-мьютекс уронил бы .unwrap() прямо здесь и повесил выход
                // (см. util::LockExt и заметку про exit-hang).
                let minimize = app
                    .state::<AppState>()
                    .settings
                    .lock_recover()
                    .minimize_to_tray;
                if minimize {
                    // Сворачиваем в трей вместо выхода. Немедленно сообщаем фронту,
                    // что окно скрыто; поллер (см. setup) всё равно продублирует —
                    // но так пауза анимаций срабатывает без задержки.
                    api.prevent_close();
                    let _ = window.hide();
                    let _ = app.emit("window-visibility", false);
                } else {
                    // Полное закрытие (не сворачивание). КРИТИЧНО: teardown
                    // (taskkill + join «Глаз») НЕ делаем прямо здесь — этот
                    // обработчик крутится на главном потоке событийного цикла, и
                    // блокирующая работа в нём вешает teardown WebView2/окна на
                    // Windows (окно «зависало намертво», пока запущен обход).
                    // Предотвращаем закрытие и уводим весь выход в фоновый поток.
                    api.prevent_close();
                    begin_exit(&app);
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
            commands::dpi_engine_list,
            commands::dpi_zapret2_profiles,
            commands::dpi_engine_set,
            commands::dpi_test_cancel,
            commands::dpi_detect_orphaned,
            commands::dpi_emergency_kill,
            commands::get_network_identity,
            commands::get_netcache_stats,
            commands::record_working_config,
            commands::proxy_available,
            commands::proxy_start,
            commands::proxy_stop,
            commands::proxy_close_lan,
            commands::proxy_link,
            commands::open_external_url,
            commands::hosts_status,
            commands::hosts_install,
            commands::hosts_check,
            commands::hosts_uninstall,
            commands::hosts_restore,
            commands::runtime_get_snapshot,
            commands::bootstrap_get_snapshot,
            commands::get_settings,
            commands::update_settings,
            commands::set_hotkey,
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
            commands::legacy_reliability_approve,
            commands::adaptive_get_status,
            commands::adaptive_start_search,
            commands::adaptive_get_recommendation,
            commands::adaptive_apply_recommendation,
            commands::adaptive_cancel_search,
            commands::adaptive_confirm_candidate,
            commands::adaptive_reject_candidate,
            commands::adaptive_reset_saved,
            onboarding::onboarding_start,
            onboarding::onboarding_get_snapshot,
            onboarding::onboarding_check_readiness,
            onboarding::onboarding_save_draft,
            onboarding::onboarding_build_plan,
            onboarding::onboarding_apply,
            onboarding::onboarding_get_transaction,
            onboarding::onboarding_verify,
            onboarding::onboarding_accept_verification,
            onboarding::onboarding_rollback,
            onboarding::onboarding_complete,
            onboarding::onboarding_skip,
            onboarding::onboarding_cancel,
            onboarding::launch_repair_setup,
        ])
        .build(tauri::generate_context!())
        .expect("ошибка запуска Obsession")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                // Внешний exit сначала переводим в наш gate-aware teardown. Когда
                // `begin_exit` позднее вызовет app.exit(0), флаг уже выставлен и
                // второй ExitRequested свободно завершит event loop.
                if !SHUTTING_DOWN.load(Ordering::SeqCst) {
                    api.prevent_exit();
                    begin_exit(app_handle);
                }
            }
        });
}

/// Запускает завершение приложения, НЕ блокируя главный поток (event loop).
fn begin_exit(app: &tauri::AppHandle) {
    if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    app.state::<AppState>()
        .shutting_down
        .store(true, Ordering::SeqCst);

    // Мгновенный визуальный отклик: прячем окно, пока teardown ждёт operation
    // gates и завершает внешние процессы в background runtime task.
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Последняя страховка от зависшего драйвера/внешнего процесса. Делает
        // exit ТОЛЬКО если shutdown не завершился штатно за 15с — иначе поток
        // убивал бы процесс посреди teardown WebView2 даже при нормальном выходе.
        static SHUTDOWN_DONE: AtomicBool = AtomicBool::new(false);
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(15));
            if !SHUTDOWN_DONE.load(Ordering::SeqCst) {
                std::process::exit(0);
            }
        });
        shutdown(&app).await;
        // Хвост лога сбрасываем последним: LogSink живёт в static, деструкторы
        // статиков при завершении процесса не выполняются, и без этого вызова
        // последние строки teardown терялись вместе с буфером.
        util::flush_log_file();
        SHUTDOWN_DONE.store(true, Ordering::SeqCst);
        app.exit(0);
    });
}

/// Gate-aware teardown. Флаг shutdown выставлен до входа сюда, поэтому новые
/// start/test уже отклоняются. Сначала ждём незавершённые операции, затем гасим
/// Brain/Eyes и дочерние процессы. На главном потоке эта функция не выполняется.
async fn shutdown(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();

    // Coordinator может держать временный candidate и для Shutdown обязан
    // сначала выполнить exact rollback. Поэтому ждём его ДО захвата dpi_gate.
    let adaptive = state
        .adaptive
        .lock()
        .ok()
        .and_then(|mut value| value.take());
    if let Some(handle) = adaptive {
        handle.shutdown().await;
    }

    let _dpi_gate = state.dpi_gate.lock().await;
    let _proxy_gate = state.proxy_gate.lock().await;
    // hosts_gate ждём последним, сохраняя единственное существующее направление
    // вложенности (dpi → proxy → hosts): обратного порядка нет ни в одном месте
    // дерева, поэтому цикла блокировок это не создаёт.
    //
    // Ждать обязательно: install() между подменой системного hosts и записью
    // состояния делает flush_dns и до 6 сетевых проб. Выход в этом окне
    // обрывал транзакцию на полпути, а точка возврата ещё не была на диске.
    let _hosts_gate = state.hosts_gate.lock().await;

    let brain = state.brain.lock().ok().and_then(|mut b| b.take());
    if let Some(handle) = brain {
        handle.shutdown();
    }

    // stop_all сначала будит WinDivert recv и join-ит Eyes, затем завершает все
    // tracked winws. PID регистрируются сразу после spawn, поэтому окно orphan
    // между spawn и ранней проверкой закрыто.
    if let Err(error) = protected_runtime::dpi_stop(app).await {
        util::emit_log(
            app,
            "error",
            "dpi",
            &format!("Не удалось остановить защищённый DPI runtime при выходе: {error}"),
        );
    }
    // Уже держим proxy_gate: используем locked-вариант без повторного lock.
    proxy::stop_locked_async(app).await;
}

/// Строит иконку в системном трее с меню Показать/Выход.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let dpi_runtime_available = protected_runtime::dpi_available();
    let legacy_runtime_available = security::protected_runtime_available();
    let dpi_label = if dpi_runtime_available {
        "DPI-обход"
    } else {
        "DPI-обход (временно отключён)"
    };
    let proxy_label = if legacy_runtime_available {
        "Telegram-прокси"
    } else {
        "Telegram-прокси (временно отключён)"
    };
    let dpi = CheckMenuItem::with_id(
        app,
        "toggle_dpi",
        dpi_label,
        dpi_runtime_available,
        false,
        None::<&str>,
    )?;
    let proxy = CheckMenuItem::with_id(
        app,
        "toggle_proxy",
        proxy_label,
        legacy_runtime_available,
        false,
        None::<&str>,
    )?;
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
                    let _ = app.emit("window-visibility", true);
                }
            }
            "quit" => begin_exit(app),
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
                    let _ = app.emit("window-visibility", true);
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

    let (idle_path, active_path) = {
        let st = app.state::<AppState>();
        (st.paths.tray_icon_path(), st.paths.tray_active_icon_path())
    };
    let tray = builder.build(app)?;
    app.manage(TrayState {
        tray,
        idle_path,
        active_path,
        dpi: AtomicBool::new(false),
        proxy: AtomicBool::new(false),
    });
    Ok(())
}

/// Переключает DPI-обход из трея: конфиги берутся из сохранённых настроек
/// (с откатом к дефолтному конфигу категории, если явный выбор пуст).
fn toggle_dpi(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let configs = {
            let st = app.state::<AppState>();
            let s = st.settings.lock_recover();
            let selected_categories = if s.dpi_engine == "zapret2" {
                &s.zapret2_selected_categories
            } else {
                &s.selected_categories
            };
            let cats = if selected_categories.is_empty() {
                vec!["discord".to_string()]
            } else {
                selected_categories.clone()
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

        match commands::dpi_toggle_session(&app, configs).await {
            Ok(active) => util::notify_now(
                &app,
                "Obsession",
                if active {
                    "Защита включена"
                } else {
                    "Защита выключена"
                },
            ),
            Err(e) => {
                util::emit_log(&app, "error", "dpi", &e);
                util::notify_now(&app, "Obsession", "Не удалось переключить защиту");
            }
        }
    });
}

/// Переключает Telegram-прокси из трея: порт/домен — из сохранённых настроек.
fn toggle_proxy(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (port, domain) = {
            let st = app.state::<AppState>();
            let s = st.settings.lock_recover();
            (s.proxy_port, s.fake_tls_domain.clone())
        };
        if let Err(e) = proxy::toggle(&app, port, &domain).await {
            util::emit_log(&app, "error", "proxy", &e);
            util::notify_now(&app, "Obsession", "Не удалось переключить Telegram-прокси");
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
