//! Снижение потребления RAM веб-вью, пока окно живёт в трее. Не трогает ни
//! визуал, ни JS: страница НЕ suspend-ится, события не теряются, ресинк не нужен.
//!
//! Две независимые меры, обе безопасны:
//!  1) `ICoreWebView2_19::MemoryUsageTargetLevel` = LOW при скрытии в трей и
//!     NORMAL при показе. LOW просит Chromium реально освободить часть heap
//!     (а не просто вытолкнуть страницы в pagefile). Микрософт не советует
//!     смешивать это с `TrySuspend`/`Resume` — мы и не смешиваем.
//!  2) `EmptyWorkingSet` по нашему процессу и потомкам-`msedgewebview2.exe`
//!     через ~1.5с после скрытия — отдаёт физические страницы ОС, пока
//!     приложение простаивает в трее (рендер уже на паузе, CPU ~0%).
//!
//! `IsVisible=false` УЖЕ выставляет фронт (`getCurrentWebview().hide()`), поэтому
//! второй раз его здесь НЕ трогаем: повторный тоггл `SetIsVisible` течёт GDI
//! (WebView2Feedback #5536).

use tauri::{AppHandle, Manager};

/// Ставит `MemoryUsageTargetLevel` веб-вью: LOW (свёрнуто) или NORMAL (показано).
/// Best-effort: тихо выходит, если окна/интерфейса нет.
pub fn set_low_memory(app: &AppHandle, low: bool) {
    #[cfg(windows)]
    {
        let Some(win) = app.get_webview_window("main") else {
            return;
        };
        // Замыкание исполнится на потоке веб-вью (with_webview диспатчит сам).
        let _ = win.with_webview(move |pw| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            };
            use windows_core::Interface;

            let controller = pw.controller();
            if let Ok(core) = controller.CoreWebView2() {
                if let Ok(core19) = core.cast::<ICoreWebView2_19>() {
                    let level = if low {
                        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                    } else {
                        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                    };
                    let _ = core19.SetMemoryUsageTargetLevel(level);
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (app, low);
    }
}

/// Сбрасывает рабочий набор нашего процесса и всех потомков-`msedgewebview2.exe`
/// (браузерный/GPU/рендер-процессы веб-вью). Ограничено СВОИМ поддеревом, чтобы
/// не трогать чужие WebView2-приложения и наши winws/прокси.
#[cfg(windows)]
pub fn trim_working_set() {
    use std::collections::{HashMap, HashSet};

    use windows::Win32::Foundation::{CloseHandle, FALSE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA,
    };

    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        // (pid, parent_pid, exe_name_lower)
        let mut procs: Vec<(u32, u32, String)> = Vec::new();
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let n = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..n]).to_ascii_lowercase();
                procs.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);

        let me = GetCurrentProcessId();
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut name_of: HashMap<u32, String> = HashMap::new();
        for (pid, ppid, name) in &procs {
            children.entry(*ppid).or_default().push(*pid);
            name_of.insert(*pid, name.clone());
        }

        // Обходим дерево потомков нашего процесса (BFS/DFS).
        let mut descendants: HashSet<u32> = HashSet::new();
        let mut stack = vec![me];
        while let Some(p) = stack.pop() {
            if let Some(kids) = children.get(&p) {
                for &k in kids {
                    if descendants.insert(k) {
                        stack.push(k);
                    }
                }
            }
        }

        // Цель: сам процесс + потомки-веб-вью. winws/прокси НЕ трогаем — они
        // активны, EmptyWorkingSet им только заставит перечитать страницы.
        let mut targets: Vec<u32> = vec![me];
        for pid in &descendants {
            if name_of.get(pid).map(|n| n == "msedgewebview2.exe").unwrap_or(false) {
                targets.push(*pid);
            }
        }

        for pid in targets {
            if let Ok(h) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_SET_QUOTA, FALSE, pid) {
                if !h.is_invalid() {
                    let _ = EmptyWorkingSet(h);
                    let _ = CloseHandle(h);
                }
            }
        }
    }
}

/// На не-Windows (напр. `tauri dev` под Linux) — no-op.
#[cfg(not(windows))]
pub fn trim_working_set() {}
