//! Fail-closed граница для устаревших прямых runtime-путей внутри UI-процесса.
//!
//! Текущий code-bearing runtime ставится только в защищённый Program Files и
//! запускается службой. Ничего исполняемого из AppData этот модуль не разрешает;
//! старые прямые вызовы остаются закрытыми, пока их не удалит отдельная миграция.

/// Стабильный префикс ошибки для frontend и журналов.
pub const PROTECTED_RUNTIME_UNAVAILABLE_CODE: &str = "secure_runtime_unavailable";

/// Устаревшие прямые code-bearing пути всегда закрыты. Доступ к установленной
/// службе проверяется отдельно через `protected_runtime`.
pub const fn protected_runtime_available() -> bool {
    false
}

/// Блокирует операцию до чтения/исполнения code-bearing ресурсов из AppData.
pub fn require_protected_runtime(feature: &str) -> Result<(), String> {
    if protected_runtime_available() {
        return Ok(());
    }

    Err(format!(
        "{PROTECTED_RUNTIME_UNAVAILABLE_CODE}: {feature} временно отключён: безопасный системный компонент Obsession ещё не установлен. Приложение продолжает работать без UAC."
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn hotfix_fails_closed_until_the_protected_runtime_exists() {
        assert!(!super::protected_runtime_available());
        let error = super::require_protected_runtime("DPI").unwrap_err();
        assert!(error.starts_with(super::PROTECTED_RUNTIME_UNAVAILABLE_CODE));
        assert!(error.contains("DPI"));
    }
}
