//! Редактор пользовательских списков доменов/IP в `%APPDATA%\Obsession\lists\*.txt`.
//! Конфиги Zapret (winws) ссылаются на эти файлы относительными путями, поэтому
//! правка списка = кастомизация того, какие ресурсы попадают под обход.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Максимальный размер сохраняемого списка (5 МБ) — защита от случайной вставки
/// гигантского текста. ipset-списки (~540 КБ) с запасом укладываются.
const MAX_LIST_BYTES: usize = 5 * 1024 * 1024;

/// Сводка по одному списку для левой колонки UI.
#[derive(Serialize)]
pub struct ListInfo {
    /// Имя без расширения (оно же идентификатор для команд).
    pub name: String,
    /// Число значимых строк (без пустых и комментариев `#`).
    pub entries: usize,
    /// Размер файла в байтах.
    pub bytes: u64,
    /// "ipset" для больших списков IP-диапазонов, иначе "domains".
    pub kind: String,
}

/// Валидирует имя списка и возвращает путь к `<lists>/<name>.txt`.
/// Разрешены только `[A-Za-z0-9_-]`, чтобы исключить traversal и подкаталоги.
fn list_path(lists_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Пустое имя списка".into());
    }
    if trimmed.len() > 64 {
        return Err("Слишком длинное имя списка".into());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Имя может содержать только латиницу, цифры, _ и -".into());
    }
    Ok(lists_dir.join(format!("{trimmed}.txt")))
}

fn is_ipset(name: &str) -> bool {
    name.starts_with("ipset")
}

/// Считает значимые строки (не пустые и не начинающиеся с `#`).
fn count_entries(content: &str) -> usize {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .count()
}

/// Собирает сводку по всем `.txt`-спискам в директории.
pub fn list_all(lists_dir: &Path) -> Vec<ListInfo> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(lists_dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("txt") {
                continue;
            }
            let Some(name) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
            // Для больших ipset-файлов не читаем содержимое ради счётчика —
            // это десятки тысяч строк; показываем entries=0 как «н/д».
            let entries = if is_ipset(name) {
                0
            } else {
                std::fs::read_to_string(&p)
                    .map(|c| count_entries(&c))
                    .unwrap_or(0)
            };
            out.push(ListInfo {
                name: name.to_string(),
                entries,
                bytes,
                kind: if is_ipset(name) { "ipset" } else { "domains" }.to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Читает содержимое списка целиком.
pub fn read_list(lists_dir: &Path, name: &str) -> Result<String, String> {
    let path = list_path(lists_dir, name)?;
    std::fs::read_to_string(&path).map_err(|e| format!("Не удалось прочитать список: {e}"))
}

/// Атомарно сохраняет содержимое списка (temp + rename, fallback на прямую запись).
pub fn save_list(lists_dir: &Path, name: &str, content: &str) -> Result<(), String> {
    if content.len() > MAX_LIST_BYTES {
        return Err("Список слишком большой (> 5 МБ)".into());
    }
    // Формат проверяем ДО записи: битые записи (URL/пробелы/мусор в CIDR) попали
    // бы в файл, который winws читает построчно, и молча ломали бы фильтр.
    // Тип определяем по имени: ipset* → CIDR/IP, остальное → домены.
    let kind = crate::lists_validate::ListKind::from_name(name);
    crate::lists_validate::validate_list(content, kind).into_result()?;
    let path = list_path(lists_dir, name)?;
    let tmp = path.with_extension("txt.tmp");
    let write = std::fs::write(&tmp, content.as_bytes()).and_then(|_| std::fs::rename(&tmp, &path));
    if write.is_err() {
        let _ = std::fs::remove_file(&tmp);
        std::fs::write(&path, content.as_bytes())
            .map_err(|e| format!("Не удалось сохранить список (нет прав?): {e}"))?;
    }
    Ok(())
}

/// Создаёт новый пустой список. Ошибка, если файл уже есть.
pub fn create_list(lists_dir: &Path, name: &str) -> Result<Vec<ListInfo>, String> {
    let path = list_path(lists_dir, name)?;
    if path.exists() {
        return Err("Список с таким именем уже существует".into());
    }
    std::fs::write(&path, b"").map_err(|e| format!("Не удалось создать список: {e}"))?;
    Ok(list_all(lists_dir))
}

/// Удаляет список по имени. Возвращает актуальный набор.
pub fn delete_list(lists_dir: &Path, name: &str) -> Result<Vec<ListInfo>, String> {
    let path = list_path(lists_dir, name)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Не удалось удалить список: {e}"))?;
    }
    Ok(list_all(lists_dir))
}
