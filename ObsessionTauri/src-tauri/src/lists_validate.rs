//! Валидатор пользовательских списков доменов/IP (WS6).
//!
//! `save_list` раньше принимал ЛЮБОЙ контент до 5 МБ — вставка мусора попадала в
//! файлы, которые читает winws. Здесь — чистая проверка формата: домены (по
//! одному в строке, с поддоменами) либо ipset-CIDR. Отсекаем shell/URL/управляющие
//! символы, но толерантны к комментариям `#` и пустым строкам. Чистый модуль,
//! тестируется без FS.

#![allow(dead_code)] // is_valid — часть публичного API отчёта, используется в тестах

use std::net::IpAddr;

/// Вид списка определяет правила валидации значимых строк.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    /// Домены (discord.txt, white-list.txt и т.п.).
    Domains,
    /// IP/CIDR (ipset-*.txt).
    Ipset,
}

impl ListKind {
    /// По имени списка: `ipset*` → Ipset, иначе Domains (как в lists.rs::is_ipset).
    pub fn from_name(name: &str) -> ListKind {
        if name.starts_with("ipset") {
            ListKind::Ipset
        } else {
            ListKind::Domains
        }
    }
}

/// Отчёт валидации. `errors` пуст ⇔ список можно сохранять.
#[derive(Debug, Default)]
pub struct ListValidationReport {
    pub errors: Vec<String>,
    /// Число значимых записей (не пустых, не комментариев).
    pub entries: usize,
}

impl ListValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Свёртка в `Result` для вызова из `save_list`: Ok если валиден, иначе первая
    /// ошибка + число остальных.
    pub fn into_result(self) -> Result<(), String> {
        if self.errors.is_empty() {
            return Ok(());
        }
        let first = self.errors[0].clone();
        let more = self.errors.len().saturating_sub(1);
        if more > 0 {
            Err(format!("{first} (и ещё {more})"))
        } else {
            Err(first)
        }
    }
}

const MAX_ERRORS: usize = 50;

/// Проверяет содержимое списка. `kind` задаёт правила значимых строк.
pub fn validate_list(content: &str, kind: ListKind) -> ListValidationReport {
    let mut report = ListValidationReport::default();

    if content.contains('\0') {
        report.errors.push("список содержит NUL-байт".into());
        return report;
    }

    for (idx, raw) in content.lines().enumerate() {
        let ln = idx + 1;
        // Управляющие символы (кроме таба) недопустимы в списке.
        if raw.chars().any(|c| c.is_control() && c != '\t') {
            push(&mut report, format!("строка {ln}: управляющий символ"));
            continue;
        }
        let line = strip_inline_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let ok = match kind {
            ListKind::Domains => validate_domain_line(line),
            ListKind::Ipset => validate_ipset_line(line),
        };
        match ok {
            Ok(()) => report.entries += 1,
            Err(reason) => push(&mut report, format!("строка {ln}: {reason} ({line:?})")),
        }
    }

    report
}

fn push(report: &mut ListValidationReport, msg: String) {
    if report.errors.len() < MAX_ERRORS {
        report.errors.push(msg);
    } else if report.errors.len() == MAX_ERRORS {
        report.errors.push("… дополнительные ошибки опущены".into());
    }
}

/// Отрезает хвостовой inline-комментарий `# ...` (в gaming.txt так помечают
/// опции). Символ `#` в начале уже обрабатывается вызывающим как коммент-строка.
fn strip_inline_comment(line: &str) -> &str {
    match line.find('#') {
        Some(pos) => &line[..pos],
        None => line,
    }
}

/// Домен: буквы/цифры/`.`/`-`/`_`, без URL/shell/пробелов. Допускаем ведущую
/// точку (`.example.com` — все поддомены).
fn validate_domain_line(d: &str) -> Result<(), &'static str> {
    if d.contains("://") {
        return Err("похоже на URL");
    }
    if d.split_whitespace().count() != 1 {
        return Err("несколько токенов в строке");
    }
    let core = d.strip_prefix('.').unwrap_or(d);
    if core.is_empty() {
        return Err("пустой домен");
    }
    for ch in core.chars() {
        if !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_') {
            return Err("недопустимый символ в домене");
        }
    }
    if core.starts_with('-') || core.ends_with('-') || !core.contains('.') {
        return Err("некорректная форма домена");
    }
    Ok(())
}

/// ipset-строка: либо одиночный IP, либо CIDR `ip/prefix`.
fn validate_ipset_line(s: &str) -> Result<(), &'static str> {
    if s.split_whitespace().count() != 1 {
        return Err("несколько токенов в строке");
    }
    if let Some((ip, prefix)) = s.split_once('/') {
        let ip: IpAddr = ip.parse().map_err(|_| "невалидный IP в CIDR")?;
        let max = if ip.is_ipv6() { 128 } else { 32 };
        let p: u8 = prefix.parse().map_err(|_| "невалидный префикс")?;
        if p as u16 > max {
            return Err("префикс вне диапазона");
        }
        Ok(())
    } else {
        s.parse::<IpAddr>().map(|_| ()).map_err(|_| "невалидный IP")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_domain_list_with_comments() {
        let content = "# заголовок\ndiscord.com\ncdn.discordapp.com\n.example.org\n\n";
        let r = validate_list(content, ListKind::Domains);
        assert!(r.is_valid(), "errors: {:?}", r.errors);
        assert_eq!(r.entries, 3);
    }

    #[test]
    fn accepts_inline_comment_like_gaming_txt() {
        // gaming.txt: домен, затем строка с хвостовым комментарием.
        let content = "unrealengine.com   # Epic\nfn.gg\n";
        let r = validate_list(content, ListKind::Domains);
        assert!(r.is_valid(), "errors: {:?}", r.errors);
        assert_eq!(r.entries, 2);
    }

    #[test]
    fn rejects_url_and_shell_in_domain() {
        let url = validate_list("http://evil.com/x\n", ListKind::Domains);
        assert!(!url.is_valid());
        let shell = validate_list("a.com;rm -rf $HOME\n", ListKind::Domains);
        assert!(!shell.is_valid());
    }

    #[test]
    fn rejects_nul_and_control_chars() {
        assert!(!validate_list("a.com\n\0\n", ListKind::Domains).is_valid());
        assert!(!validate_list("a.com\n\x07bad\n", ListKind::Domains).is_valid());
    }

    #[test]
    fn accepts_valid_ipset() {
        let content = "1.0.0.0/24\n8.8.8.8\n2606:4700::/32\n";
        let r = validate_list(content, ListKind::Ipset);
        assert!(r.is_valid(), "errors: {:?}", r.errors);
        assert_eq!(r.entries, 3);
    }

    #[test]
    fn rejects_bad_cidr() {
        assert!(!validate_list("1.2.3.0/40\n", ListKind::Ipset).is_valid());
        assert!(!validate_list("not.an.ip/24\n", ListKind::Ipset).is_valid());
        assert!(!validate_list("999.1.1.1\n", ListKind::Ipset).is_valid());
    }

    #[test]
    fn kind_from_name() {
        assert_eq!(ListKind::from_name("ipset-gaming"), ListKind::Ipset);
        assert_eq!(ListKind::from_name("discord"), ListKind::Domains);
    }

    #[test]
    fn empty_content_is_valid_zero_entries() {
        let r = validate_list("\n\n# только коммент\n", ListKind::Domains);
        assert!(r.is_valid());
        assert_eq!(r.entries, 0);
    }
}
