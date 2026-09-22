//! Isolated WebView2 lifecycle check; does not run Obsession's DPI backend.
//! Start Vite, then `cargo run --release --example tray_memory_probe`.
#[path = "../src/webmem.rs"]
mod webmem;
use tauri::Manager;
use std::time::Duration;

fn sample(app: &tauri::AppHandle, label: &'static str) {
    let win = app.get_webview_window("main").unwrap();
    win.with_webview(move |pw| unsafe {
        use windows_core::Interface;
        use webview2_com::{ExecuteScriptCompletedHandler, Microsoft::Web::WebView2::Win32::ICoreWebView2_3};
        let core = pw.controller().CoreWebView2().unwrap();
        let core3 = core.cast::<ICoreWebView2_3>().unwrap();
        let mut asleep = windows_core::BOOL::default();
        core3.IsSuspended(&mut asleep).unwrap();
        println!("{label}: suspended={}", asleep.as_bool());
        let code: Vec<u16> = "JSON.stringify({ticks:document.documentElement.dataset.ticks,hidden:document.hidden,canvases:document.querySelectorAll('canvas').length,report:document.querySelector('#report')?.textContent})\0".encode_utf16().collect();
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |result, json| { println!("{label}: {result:?} {json}"); Ok(()) }));
        core.ExecuteScript(windows_core::PCWSTR(code.as_ptr()), &handler).unwrap();
    }).unwrap();
}

fn main() {
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "com.vlarpsu.tray-probe".into();
    context.config_mut().app.windows.clear();
    tauri::Builder::default().setup(|app| {
        tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External("http://127.0.0.1:1420/artifacts/theme-memory/tray.html".parse().unwrap()))
            .title("Obsession isolated tray probe").inner_size(1000.0, 680.0)
            .data_directory(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/tray-probe-webview"))
            .build()?;
        println!("probe pid={}", std::process::id());
        let handle = app.handle().clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(8));
            for cycle in 0..2 {
                let win = handle.get_webview_window("main").unwrap();
                win.eval("window.trayProbe.render('goldenmeadow')").unwrap();
                std::thread::sleep(Duration::from_secs(6));
                sample(&handle, "visible");
                win.eval("window.trayProbe.shown(false)").unwrap();
                win.hide().unwrap();
                std::thread::sleep(Duration::from_secs(3));
                webmem::set_low_memory(&handle, true);
                std::thread::sleep(Duration::from_secs(10));
                sample(&handle, "tray");
                std::thread::sleep(Duration::from_secs(5));
                webmem::set_low_memory(&handle, false);
                win.show().unwrap();
                win.eval("window.trayProbe.shown(true); window.trayProbe.render('ophanim')").unwrap();
                std::thread::sleep(Duration::from_secs(5));
                sample(&handle, "restored");
                println!("cycle {cycle} complete");
            }
            handle.get_webview_window("main").unwrap().navigate("http://127.0.0.1:1420/artifacts/theme-memory/index.html".parse().unwrap()).unwrap();
            std::thread::sleep(Duration::from_secs(10));
            sample(&handle, "sprites");
            std::thread::sleep(Duration::from_secs(2));
            handle.exit(0);
        });
        Ok(())
    }).run(context).expect("probe failed");
}
