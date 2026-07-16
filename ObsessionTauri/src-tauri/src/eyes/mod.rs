//! «Глаза» — пассивный наблюдатель трафика для детекта состояния DPI-обхода.
//!
//! Архитектура (см. этап 2): нижний слой контура надёжности. Через WinDivert в
//! режиме `SNIFF | RECV_ONLY` (v1) слушает копию трафика к хостам из хостлиста и
//! эмитит per-flow сырые вердикты `working / reset / blackhole`. Агрегацию и
//! гистерезис делает «мозг» выше по стеку — здесь только наблюдение.
//!
//! Слои:
//! - [`parse`]       — чистый разбор TCP/TLS/SNI (без WinDivert/Tauri).
//! - [`fake_filter`] — отсев собственных fake-инъекций winws.
//! - [`flow`]        — автомат потока: reset/blackhole-дискриминация, пересборка CH.
//! - [`signal`]      — выходной `Observation` (сериализуется во фронт).
//! - `capture`       — WinDivert sniff-хендл (v1, только Windows) — ещё не подключён.
//!
//! Порядок постройки: **v0 — чистое ядро (`parse`+`flow`+`fake_filter`+`signal`)**,
//! тестируется на фикстурах через [`replay`] без драйвера и админ-прав. v1 добавит
//! `capture` и эмиссию в React; v2 — точность (TTL-baseline, эвикция под нагрузкой).

// Ядро «глаз» ещё не подключено к жизненному циклу приложения (запуск при
// старте winws + эмиссия в React — следующий шаг v1), поэтому публичное API
// и ре-экспорты пока «не используются». Снимем оба allow при wire-up.
#![allow(dead_code)]
#![allow(unused_imports)]

#[cfg(windows)]
pub mod capture;
pub mod fake_filter;
pub mod flow;
pub mod parse;
pub mod quic;
pub mod signal;

#[cfg(test)]
mod replay;

pub use flow::{Config, FlowTable};
pub use quic::{classify_udp, UdpKind};
pub use signal::{Diagnosis, Observation, Verdict};

#[cfg(windows)]
pub use capture::{start, EyesHandle};
