//! Чистый валидатор hosts-payload (Malw/GeoHide) — WS1 задача 1.2.
//!
//! Работает над СЫРЫМИ БАЙТАМИ (Foundation F.1): никакой lossy-конверсии до
//! проверок, чтобы дефект кодировки/BOM не «просочился» в системный `hosts`. Без
//! `AppHandle`/сети/FS — детерминированно тестируется, путь не зашит.
//!
//! Контракт: `download_hosts` только тянет байты; преобразование в применимый
//! файл выполняется ТОЛЬКО после успешного [`validate_hosts_payload`]. Валидатор
//! отдаёт нормализованный (LF, без BOM) буфер, готовый к атомарной записи.

#![allow(dead_code)] // подключается к транзакции apply в WS1 задача 1.5

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

/// Лимиты и требования валидации. Провайдер/путь не зашиты — их задаёт вызывающий.
#[derive(Clone, Debug)]
pub struct ValidationLimits {
    /// Максимальный размер payload в байтах.
    pub max_bytes: usize,
    /// Домены, которые ОБЯЗАНЫ присутствовать (AI-сервисы выбранного провайдера).
    /// Пусто = не проверять минимальный набор.
    pub required_domains: Vec<String>,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            // Соответствует MAX_HOSTS_BYTES в hosts.rs.
            max_bytes: 10 * 1024 * 1024,
            required_domains: Vec::new(),
        }
    }
}

/// Результат валидации. `errors` пуст ⇔ payload валиден и `normalized` можно писать.
#[derive(Debug, Default)]
pub struct HostsValidationReport {
    pub errors: Vec<String>,
    /// Число валидных hosts-записей (строк IP→домены).
    pub entry_count: usize,
    /// Комментарий версии (`# Последнее обновление: …` / `# update: …`), если найден.
    pub version_comment: Option<String>,
    /// Нормализованный к LF, BOM-free payload. Валиден к записи только при пустом `errors`.
    pub normalized: Vec<u8>,
    /// Все домены записей (lowercase) — для внешней сверки / диагностики.
    pub domains: BTreeSet<String>,
}

impl HostsValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Максимум сообщений об ошибках в отчёте (защита от 30k-строчного payload).
const MAX_ERRORS: usize = 100;

/// Проверяет payload и возвращает отчёт. Чистая функция: те же байты → тот же отчёт.
pub fn validate_hosts_payload(raw: &[u8], limits: &ValidationLimits) -> HostsValidationReport {
    let mut report = HostsValidationReport::default();

    // --- Байтовые предохранители (до любой конверсии) ---
    if raw.is_empty() {
        report.errors.push("пустой payload".into());
        return report;
    }
    if raw.len() > limits.max_bytes {
        report.errors.push(format!(
            "payload превышает лимит: {} > {} байт",
            raw.len(),
            limits.max_bytes
        ));
        return report;
    }
    if raw.contains(&0u8) {
        report.errors.push("payload содержит NUL-байт".into());
        return report;
    }

    // Строгая проверка кодировки — БЕЗ lossy (иначе битые байты попадут в hosts).
    let text = match std::str::from_utf8(raw) {
        Ok(t) => t,
        Err(e) => {
            report.errors.push(format!(
                "недопустимая кодировка (не UTF-8), первый плохой байт на позиции {}",
                e.valid_up_to()
            ));
            return report;
        }
    };

    // BOM + нормализация переводов строк (семантику строк не меняем).
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let normalized = normalize_newlines(text);
    report.normalized = normalized.clone().into_bytes();

    // --- Построчный разбор ---
    // Ключ конфликта = (домен, семейство): dual-stack (v4+v6) конфликтом НЕ считаем,
    // а два разных v4 (или два разных v6) для одного домена — считаем.
    let mut by_key: BTreeMap<(String, bool), IpAddr> = BTreeMap::new();

    for (idx, line) in normalized.lines().enumerate() {
        let ln = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            if report.version_comment.is_none() {
                if let Some(v) = extract_version(trimmed) {
                    report.version_comment = Some(v);
                }
            }
            continue;
        }

        // Запись: IP + один-или-более доменов.
        let mut parts = trimmed.split_whitespace();
        let ip_tok = parts.next().unwrap_or("");
        let domains: Vec<&str> = parts.collect();

        let ip: IpAddr = match ip_tok.parse() {
            Ok(ip) => ip,
            Err(_) => {
                // Первый токен не IP → это не hosts-запись (заголовок/секция/текст
                // источника). ТОЛЕРИРУЕМ: строка сохраняется в normalized, резолвер её
                // игнорирует. Опасное содержимое отсекается иначе: NUL/кодировка/лимит —
                // глобально выше; URL/shell в доменах записей — ниже; «весь файл мусор»
                // ловит проверка entry_count==0. Строгий reject ломал бы реальный Malw.
                continue;
            }
        };
        if domains.is_empty() {
            push_err(&mut report, format!("строка {ln}: IP без домена"));
            continue;
        }
        report.entry_count += 1;

        for d in domains {
            if let Err(reason) = validate_domain(d) {
                push_err(
                    &mut report,
                    format!("строка {ln}: недопустимый домен {d:?} ({reason})"),
                );
                continue;
            }
            let dl = d.to_ascii_lowercase();
            let key = (dl.clone(), ip.is_ipv6());
            match by_key.get(&key) {
                Some(existing) if *existing != ip => {
                    push_err(
                        &mut report,
                        format!("конфликт адреса для {dl}: {existing} и {ip}"),
                    );
                }
                _ => {
                    by_key.insert(key, ip);
                }
            }
            report.domains.insert(dl);
        }
    }

    // Ни одной hosts-записи → payload не является hosts-файлом (напр. HTML-страница
    // ошибки вместо содержимого). Не даём записать такой мусор в системный hosts.
    if report.entry_count == 0 {
        push_err(
            &mut report,
            "payload не содержит ни одной hosts-записи (IP → домен)".into(),
        );
    }

    // --- Обязательные AI-домены провайдера ---
    for req in &limits.required_domains {
        let req_l = req.to_ascii_lowercase();
        if !report.domains.contains(&req_l) {
            push_err(
                &mut report,
                format!("отсутствует обязательный AI-домен: {req}"),
            );
        }
    }

    report
}

fn push_err(report: &mut HostsValidationReport, msg: String) {
    match report.errors.len().cmp(&MAX_ERRORS) {
        std::cmp::Ordering::Less => report.errors.push(msg),
        std::cmp::Ordering::Equal => report.errors.push("… дополнительные ошибки опущены".into()),
        std::cmp::Ordering::Greater => {}
    }
}

/// CRLF → LF, одиночный CR → LF. Прочие символы не трогаем.
fn normalize_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

/// Извлекает значение версии из строки-комментария, если это версия-маркер.
fn extract_version(comment_line: &str) -> Option<String> {
    let body = comment_line.trim_start_matches('#').trim();
    for tag in ["Последнее обновление:", "update:"] {
        if let Some(rest) = body.strip_prefix(tag) {
            let v = rest.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Санитария домена: только hostname-символы. Отсекает URL, shell-метасимволы,
/// слэши, двоеточия — всё, что не место в правой части hosts-записи.
fn validate_domain(d: &str) -> Result<(), &'static str> {
    if d.is_empty() {
        return Err("пусто");
    }
    if d.contains("://") {
        return Err("похоже на URL");
    }
    for ch in d.chars() {
        if !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_') {
            return Err("недопустимый символ");
        }
    }
    if d.starts_with('.') || d.starts_with('-') || d.ends_with('-') {
        return Err("некорректная форма hostname");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> ValidationLimits {
        ValidationLimits::default()
    }

    #[test]
    fn accepts_comments_and_valid_ipv4_ipv6() {
        let raw = b"# comment\n1.2.3.4 chatgpt.com\n::1 claude.ai api.claude.ai\n";
        let r = validate_hosts_payload(raw, &limits());
        assert!(r.is_valid(), "errors: {:?}", r.errors);
        assert_eq!(r.entry_count, 2);
        assert!(r.domains.contains("claude.ai"));
        assert!(r.domains.contains("api.claude.ai"));
    }

    #[test]
    fn rejects_empty() {
        let r = validate_hosts_payload(b"", &limits());
        assert!(!r.is_valid());
    }

    #[test]
    fn rejects_over_limit() {
        let lim = ValidationLimits {
            max_bytes: 8,
            ..Default::default()
        };
        let r = validate_hosts_payload(b"1.2.3.4 example.com\n", &lim);
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("лимит")));
    }

    #[test]
    fn rejects_nul_byte() {
        let r = validate_hosts_payload(b"1.2.3.4 a.com\n\0evil\n", &limits());
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("NUL")));
    }

    #[test]
    fn rejects_invalid_encoding() {
        // 0xFF — невалидный старт UTF-8.
        let r = validate_hosts_payload(&[0x31, 0x2e, 0xff, 0x0a], &limits());
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("кодировка")));
    }

    #[test]
    fn rejects_url_and_shell_constructs() {
        let url = validate_hosts_payload(b"1.2.3.4 http://evil.com/x\n", &limits());
        assert!(!url.is_valid());
        let shell = validate_hosts_payload(b"1.2.3.4 a.com;rm$(whoami)\n", &limits());
        assert!(!shell.is_valid());
    }

    #[test]
    fn rejects_conflicting_ipv4_for_same_domain() {
        let raw = b"1.2.3.4 x.com\n5.6.7.8 x.com\n";
        let r = validate_hosts_payload(raw, &limits());
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("конфликт")));
    }

    #[test]
    fn dual_stack_v4_and_v6_is_not_conflict() {
        let raw = b"1.2.3.4 x.com\n2606:4700::1 x.com\n";
        let r = validate_hosts_payload(raw, &limits());
        assert!(r.is_valid(), "errors: {:?}", r.errors);
    }

    #[test]
    fn requires_expected_ai_domains() {
        let lim = ValidationLimits {
            required_domains: vec!["chatgpt.com".into(), "claude.ai".into()],
            ..Default::default()
        };
        let missing = validate_hosts_payload(b"1.2.3.4 chatgpt.com\n", &lim);
        assert!(!missing.is_valid());
        assert!(missing.errors.iter().any(|e| e.contains("claude.ai")));

        let present = validate_hosts_payload(b"1.2.3.4 chatgpt.com\n1.2.3.4 claude.ai\n", &lim);
        assert!(present.is_valid(), "errors: {:?}", present.errors);
    }

    #[test]
    fn preserves_version_comment() {
        let raw = "# Последнее обновление: 30 мая 2026\n1.2.3.4 a.com\n".as_bytes();
        let r = validate_hosts_payload(raw, &limits());
        assert_eq!(r.version_comment.as_deref(), Some("30 мая 2026"));
    }

    #[test]
    fn normalizes_crlf_and_strips_bom_without_changing_semantics() {
        // BOM + CRLF на входе.
        let mut raw = vec![0xEF, 0xBB, 0xBF];
        raw.extend_from_slice(b"# c\r\n1.2.3.4 a.com\r\n");
        let r = validate_hosts_payload(&raw, &limits());
        assert!(r.is_valid(), "errors: {:?}", r.errors);
        // Нормализованный буфер — LF, без BOM, семантика сохранена.
        assert_eq!(r.normalized, b"# c\n1.2.3.4 a.com\n");
        assert_eq!(r.entry_count, 1);
    }

    #[test]
    fn rejects_entry_without_domain() {
        let r = validate_hosts_payload(b"1.2.3.4\n", &limits());
        assert!(!r.is_valid());
    }

    #[test]
    fn tolerates_non_entry_header_lines() {
        // Реальные hosts содержат не-# заголовки/секции — они не валят валидацию.
        let raw = b"Malw DNS unblock list\nsome section\n1.2.3.4 chatgpt.com\n";
        let r = validate_hosts_payload(raw, &limits());
        assert!(r.is_valid(), "errors: {:?}", r.errors);
        assert_eq!(r.entry_count, 1);
    }

    #[test]
    fn rejects_payload_with_no_entries() {
        // HTML-страница ошибки вместо hosts: ни одной IP-записи → reject.
        let raw = b"<html><body>404 Not Found</body></html>\n";
        let r = validate_hosts_payload(raw, &limits());
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("ни одной hosts-записи")));
    }
}
