//! Tray-only WebView suspension. The Rust runtime and protected DPI service keep
//! running. Resume precedes the visibility event; the frontend refreshes its
//! backend snapshot on return. No working-set trimming or forced GC.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::{AppHandle, Manager};

static SLEEP_REQUESTED: AtomicBool = AtomicBool::new(false);
static RESTORE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Called after the frontend has released graphics, and immediately on restore.
/// Visibility is rechecked on the UI thread to reject a delayed hide operation.
pub fn set_low_memory(app: &AppHandle, low: bool) {
    // A late frontend idle message must never cancel a queued native restore.
    // Only restores invalidate queued sleep work; sleep becomes desired only
    // after the UI thread confirms that the native window is still hidden.
    if !low {
        SLEEP_REQUESTED.store(false, Ordering::SeqCst);
        RESTORE_GENERATION.fetch_add(1, Ordering::SeqCst);
    }
    let restore_generation = RESTORE_GENERATION.load(Ordering::SeqCst);
    #[cfg(windows)]
    {
        let Some(win) = app.get_webview_window("main") else {
            return;
        };
        let Ok(handle) = win.hwnd() else {
            return;
        };
        let raw_handle = handle.0 as usize;
        let _ = win.with_webview(move |pw| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
            use webview2_com::TrySuspendCompletedHandler;
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};
            use windows_core::Interface;

            if RESTORE_GENERATION.load(Ordering::SeqCst) != restore_generation {
                return;
            }
            let hwnd = HWND(raw_handle as _);
            let shown = IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool();
            if low && shown {
                return;
            }
            SLEEP_REQUESTED.store(low, Ordering::SeqCst);
            let controller = pw.controller();
            let Ok(core) = controller.CoreWebView2() else {
                return;
            };
            let Ok(core3) = core.cast::<ICoreWebView2_3>() else {
                return;
            };
            let mut visible = windows_core::BOOL::default();
            if controller.IsVisible(&mut visible).is_err() {
                return;
            }
            if low {
                // Set only on a transition; repeated SetIsVisible can leak GDI.
                if visible.as_bool() && controller.SetIsVisible(false).is_err() {
                    return;
                }
                let completion_core = core3.clone();
                let completion =
                    TrySuspendCompletedHandler::create(Box::new(move |result, success| {
                        if !SLEEP_REQUESTED.load(Ordering::SeqCst) {
                            let _ = completion_core.Resume();
                        }
                        if result.is_err() || !success {
                            eprintln!(
                                "WebView tray suspension was not accepted: {result:?}, {success:?}"
                            );
                        }
                        Ok(())
                    }));
                if let Err(error) = core3.TrySuspend(&completion) {
                    eprintln!("WebView tray suspension failed: {error}");
                }
            } else {
                let _ = core3.Resume();
                if !visible.as_bool() {
                    let _ = controller.SetIsVisible(true);
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}
