//! Автозапуск через `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! Состояние читается из реестра, чтобы не расходиться с ручными изменениями.
//! Запуск свёрнутым задаётся отдельно через `start_minimized`.

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "Obsession";

/// True, если запись автозапуска присутствует в реестре.
#[cfg(windows)]
pub fn is_enabled() -> bool {
    crate::util::std_command("reg")
        .args(["query", RUN_KEY, "/v", VALUE_NAME])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Включает/выключает автозапуск. Путь к exe берётся текущий; оборачиваем в
/// кавычки на случай пробелов в пути (напр. `C:\Program Files\...`).
#[cfg(windows)]
pub fn set(enable: bool) -> Result<(), String> {
    if enable {
        // В dev-сборке exe грузит devUrl (http://localhost:1420). Прописанный в
        // автозапуск, при следующем входе в систему он откроет «страница
        // недоступна» — Vite не запущен. Автозапуск имеет смысл только для
        // установленной release-сборки со встроенным dist. Не пишем битый ключ.
        if cfg!(debug_assertions) {
            return Err(
                "Автозапуск доступен только в установленной версии (не в dev-сборке).".into(),
            );
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let data = format!("\"{}\"", exe.display());
        let status = crate::util::std_command("reg")
            .args([
                "add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &data, "/f",
            ])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("не удалось записать ключ автозапуска".into());
        }
    } else {
        // Отсутствие значения при удалении — не ошибка.
        let _ = crate::util::std_command("reg")
            .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
            .status();
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(not(windows))]
pub fn set(_enable: bool) -> Result<(), String> {
    Ok(())
}
