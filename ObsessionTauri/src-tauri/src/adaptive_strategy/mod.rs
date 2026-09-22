//! Локальный безопасный подбор Strategy Pack для Zapret2.
//!
//! Модуль намеренно отделён от существующего `brain`: текущий Brain управляет
//! проверенной Legacy-лестницей, а adaptive-контур сначала строится как чистое
//! unit-tested ядро без side effects и доступа к DPI runtime.

pub mod cache;
mod candidate_runtime;
pub mod compiler;
pub mod dsl;
pub mod evidence;
pub mod generator;
mod http_probe;
pub mod model;
pub mod probe;
pub mod recommendation;
mod rollback;
pub mod runtime;
mod tasks;
pub mod validator;
