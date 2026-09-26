//! Геометрия главного окна: размеры свёрнутого окна не заменяют нормальные.

use std::sync::Mutex;

use tauri::{LogicalSize, Manager, PhysicalSize};

use crate::util::LockExt;

#[derive(Clone, Copy, Debug, PartialEq)]
struct RestoreTarget {
    size: LogicalSize<f64>,
    maximized: bool,
}

pub(crate) struct MainWindowGeometry {
    minimum: LogicalSize<f64>,
    resizable: bool,
    target: Mutex<RestoreTarget>,
}

impl MainWindowGeometry {
    pub(crate) fn new(config: &tauri::utils::config::WindowConfig) -> Self {
        let minimum = LogicalSize::new(
            config.min_width.unwrap_or(800.0),
            config.min_height.unwrap_or(600.0),
        );
        Self {
            minimum,
            resizable: config.resizable,
            target: Mutex::new(RestoreTarget {
                size: LogicalSize::new(
                    config.width.max(minimum.width),
                    config.height.max(minimum.height),
                ),
                maximized: config.maximized,
            }),
        }
    }

    fn valid_size(&self, size: LogicalSize<f64>) -> bool {
        size.width.is_finite()
            && size.height.is_finite()
            && size.width + 0.5 >= self.minimum.width
            && size.height + 0.5 >= self.minimum.height
    }

    fn observe(
        &self,
        size: PhysicalSize<u32>,
        scale: f64,
        shown: bool,
        minimized: bool,
        maximized: bool,
    ) {
        if !shown || minimized || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        let size = size.to_logical(scale);
        if !self.valid_size(size) {
            return;
        }
        let mut target = self.target.lock_recover();
        target.maximized = maximized;
        if !maximized {
            target.size = size;
        }
    }
}

pub(crate) fn remember(window: &tauri::Window, size: PhysicalSize<u32>) {
    if window.label() != "main" {
        return;
    }
    let Some(geometry) = window.try_state::<MainWindowGeometry>() else {
        return;
    };
    geometry.observe(
        size,
        window.scale_factor().unwrap_or(1.0),
        window.is_visible().unwrap_or(false),
        window.is_minimized().unwrap_or(true),
        window.is_maximized().unwrap_or(false),
    );
}

/// Вызывается на главном потоке, чтобы показ, восстановление и resize не расходились по очередям.
pub(crate) fn restore(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let geometry = window.state::<MainWindowGeometry>();
    let target = *geometry.target.lock_recover();
    window.show()?;
    if window.is_minimized()? {
        window.unminimize()?;
    }
    let scale = window.scale_factor()?;
    let size = window.inner_size()?.to_logical(scale);
    if !geometry.valid_size(size) {
        if window.is_maximized()? {
            window.unmaximize()?;
        }
        // Обновление нативной рамки возвращает resize даже при повреждённых стилях окна.
        if geometry.resizable {
            window.set_resizable(false)?;
        }
        window.set_resizable(geometry.resizable)?;
        window.set_min_size(Some(geometry.minimum))?;
        window.set_size(target.size)?;
        crate::util::emit_log(
            window.app_handle(),
            "warn",
            "window",
            "После сворачивания восстановлен нормальный размер окна.",
        );
    }
    if target.maximized && !window.is_maximized()? {
        window.maximize()?;
    }
    if let Ok(position) = window.outer_position() {
        if position.x < -10_000 || position.y < -10_000 {
            window.center()?;
        }
    }
    window.set_focus()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> MainWindowGeometry {
        let config: tauri::utils::config::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        MainWindowGeometry::new(
            config
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .unwrap(),
        )
    }

    #[test]
    fn tray_and_minimized_sizes_do_not_replace_the_last_normal_size() {
        let geometry = geometry();
        geometry.observe(PhysicalSize::new(1350, 975), 1.5, true, false, false);
        let saved = *geometry.target.lock_recover();
        assert_eq!(saved.size, LogicalSize::new(900.0, 650.0));
        for (size, shown, minimized) in [
            (PhysicalSize::new(0, 0), true, true),
            (PhysicalSize::new(160, 28), true, false),
            (PhysicalSize::new(1600, 900), false, false),
            (PhysicalSize::new(1600, 900), true, true),
        ] {
            geometry.observe(size, 1.0, shown, minimized, false);
            assert_eq!(*geometry.target.lock_recover(), saved);
        }
        assert!(!geometry.valid_size(LogicalSize::new(160.0, 28.0)));
    }

    #[test]
    fn maximized_restore_keeps_the_normal_size_and_ignores_minimize_events() {
        let geometry = geometry();
        geometry.observe(PhysicalSize::new(1100, 720), 1.0, true, false, false);
        geometry.observe(PhysicalSize::new(1920, 1080), 1.0, true, false, true);
        geometry.observe(PhysicalSize::new(160, 28), 1.0, true, true, false);
        assert_eq!(
            *geometry.target.lock_recover(),
            RestoreTarget {
                size: LogicalSize::new(1100.0, 720.0),
                maximized: true,
            }
        );
    }

    #[test]
    fn first_tray_restore_uses_configured_dimensions_and_dpi_limits() {
        let geometry = geometry();
        assert_eq!(
            geometry.target.lock_recover().size,
            LogicalSize::new(1000.0, 680.0)
        );
        assert_eq!(geometry.minimum, LogicalSize::new(800.0, 600.0));
        geometry.observe(PhysicalSize::new(1000, 750), 1.25, true, false, false);
        assert_eq!(
            geometry.target.lock_recover().size,
            LogicalSize::new(800.0, 600.0)
        );
        geometry.observe(PhysicalSize::new(900, 650), 0.0, true, false, false);
        assert_eq!(
            geometry.target.lock_recover().size,
            LogicalSize::new(800.0, 600.0)
        );
    }
}
