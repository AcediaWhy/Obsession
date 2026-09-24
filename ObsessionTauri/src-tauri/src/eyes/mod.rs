//! Пассивное наблюдение за трафиком DPI-обхода.
//! `capture` получает копии пакетов через WinDivert на Windows; `parse` и
//! `fake_filter` разбирают трафик и отсекают инъекции winws. `flow` формирует
//! наблюдения по соединениям, которые передаются в контур надёжности.
//! Захват запускается из `dpi.rs`. Логика разбора проверяется фикстурами в `replay`.

// Часть API доступна только на Windows и в отдельных режимах наблюдения.
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

pub use flow::{Config, FlowTable, WorkingSignalMode};
pub use quic::{classify_udp, UdpKind};
pub use signal::{Diagnosis, Observation, Verdict};

#[cfg(windows)]
pub use capture::{start, start_legacy, EyesHandle, EyesStartError};
