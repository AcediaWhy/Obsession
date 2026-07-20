//! «Мозг» (L3) — верх контура надёжности обхода. Машина состояний со строгой
//! лестницей L1→L2→L3 + ортогональный предохранитель. Повторяет паттерн Глаз:
//! чистый редуктор на логическом времени (`model`) + тонкий интерпретатор
//! сайд-эффектов (`runtime`, только под windows).
//!
//! - `model` — `Brain::step(event) -> Vec<Action>`, без tokio/WinDivert/AppHandle.
//! - `window` — скользящее окно per-flow вердиктов Глаз (агрегаты для model).
//! - `runtime` — tokio-задача: исполняет `Action`, тикает время, эмитит статус.

// Phase 1 deliberately quarantines the old global executor. Its pure model and
// compatibility runtime remain available for later migration, but no Legacy
// SessionStart reaches them while scoped Manager work is observe-only.
#![allow(dead_code)]

pub mod model;
pub mod runtime;
pub mod window;

// Наружу (dpi.rs, commands.rs) нужны только события и статус. Остальные типы
// (Brain, Action, Candidate, Source, …) используются внутри `runtime` по полному
// пути `model::…` — реэкспортировать их незачем.
pub use model::{BrainEvent, BrainStatus};
