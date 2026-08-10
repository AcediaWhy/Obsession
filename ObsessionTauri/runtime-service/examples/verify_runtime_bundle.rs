use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use obsession_runtime_protocol::{
    DpiEngine, DpiRuntimeOptions, DpiSelection, DpiStartRequest, Zapret2AdaptiveFunction,
    Zapret2AdaptiveOverride, Zapret2AdaptivePayload, Zapret2AdaptiveRange, Zapret2AdaptiveStep,
    Zapret2AdaptiveTransport, Zapret2AdaptiveValue, ZAPRET2_ADAPTIVE_SCHEMA_VERSION,
};
use obsession_runtime_service::dpi_materializer::{ProtectedDataLayout, RUNTIME_STATE_RELATIVE};
use obsession_runtime_service::protected_layout::ProtectedLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: verify_runtime_bundle <Program Files/Obsession root>")?;
    let program_files = root
        .parent()
        .ok_or("runtime bundle root has no Program Files parent")?;
    let layout = ProtectedLayout::inspect(program_files, &root)?;
    let catalog = layout.load_verified_catalog()?;
    let legacy = catalog
        .engine(DpiEngine::Legacy)
        .ok_or("runtime bundle has no Legacy engine")?;
    if legacy.strategies().is_empty() {
        return Err("runtime bundle has no Legacy strategies".into());
    }
    let zapret2 = catalog
        .engine(DpiEngine::Zapret2)
        .ok_or("runtime bundle has no Zapret2 engine")?;
    if zapret2.strategies().is_empty() {
        return Err("runtime bundle has no Zapret2 category groups".into());
    }

    let verification_root = program_files
        .parent()
        .ok_or("Program Files fixture has no verification root")?;
    let program_data = verification_root.join("ProgramData");
    let state_root = program_data.join(RUNTIME_STATE_RELATIVE);
    fs::create_dir_all(&state_root)?;
    let state = ProtectedDataLayout::inspect(&program_data, &state_root)?;

    let mut generation = 1u64;
    for strategy in legacy.strategies() {
        let plan = catalog.resolve_dpi_plan(&DpiStartRequest {
            engine: DpiEngine::Legacy,
            selections: vec![DpiSelection {
                category: strategy.category(),
                strategy_id: strategy.strategy_id().to_owned(),
            }],
            options: DpiRuntimeOptions {
                zapret2_level: 0,
                legacy_reliability: true,
                zapret2_overrides: Vec::new(),
            },
        })?;
        plan.reverify()?;
        let materialized = state.materialize(&plan, generation)?;
        if materialized.launches().len() != 1 {
            return Err(format!(
                "strategy {} materialized an unexpected launch count",
                strategy.strategy_id()
            )
            .into());
        }
        state.cleanup_generation(&materialized)?;
        generation = generation.checked_add(1).ok_or("generation overflow")?;
    }

    for strategy in zapret2.strategies() {
        let plan = catalog.resolve_dpi_plan(&DpiStartRequest {
            engine: DpiEngine::Zapret2,
            selections: vec![DpiSelection {
                category: strategy.category(),
                strategy_id: strategy.strategy_id().to_owned(),
            }],
            options: DpiRuntimeOptions {
                zapret2_level: 0,
                legacy_reliability: false,
                zapret2_overrides: Vec::new(),
            },
        })?;
        plan.reverify()?;
        let materialized = state.materialize(&plan, generation)?;
        let [launch] = materialized.launches() else {
            return Err(format!(
                "Zapret2 category group {} materialized an unexpected launch count",
                strategy.strategy_id()
            )
            .into());
        };
        if launch.response_file().is_some() || launch.arguments().is_empty() {
            return Err("Zapret2 did not produce typed service-owned arguments".into());
        }
        state.cleanup_generation(&materialized)?;
        generation = generation.checked_add(1).ok_or("generation overflow")?;

        let adaptive = Zapret2AdaptiveOverride {
            schema_version: ZAPRET2_ADAPTIVE_SCHEMA_VERSION,
            category: strategy.category(),
            transport: Zapret2AdaptiveTransport::Tls,
            steps: vec![Zapret2AdaptiveStep {
                function: Zapret2AdaptiveFunction::MultiDisorderLegacy,
                args: BTreeMap::from([(
                    "pos".into(),
                    Zapret2AdaptiveValue::Text("1,midsld".into()),
                )]),
            }],
            payload: Zapret2AdaptivePayload::TlsClientHello,
            out_range: Some(Zapret2AdaptiveRange::FirstTenDataPackets),
        };
        let plan = catalog.resolve_dpi_plan(&DpiStartRequest {
            engine: DpiEngine::Zapret2,
            selections: vec![DpiSelection {
                category: strategy.category(),
                strategy_id: strategy.strategy_id().to_owned(),
            }],
            options: DpiRuntimeOptions {
                zapret2_level: 0,
                legacy_reliability: false,
                zapret2_overrides: vec![adaptive],
            },
        })?;
        plan.reverify()?;
        let materialized = state.materialize(&plan, generation)?;
        let [launch] = materialized.launches() else {
            return Err("Adaptive Zapret2 materialized an unexpected launch count".into());
        };
        if !launch
            .arguments()
            .iter()
            .any(|argument| argument.starts_with("--name=adaptive-"))
            || !launch
                .arguments()
                .iter()
                .any(|argument| argument == "--lua-desync=multidisorder_legacy:pos=1,midsld")
        {
            return Err("Adaptive Zapret2 was not compiled by the protected service".into());
        }
        state.cleanup_generation(&materialized)?;
        generation = generation.checked_add(1).ok_or("generation overflow")?;
    }

    println!(
        "verified {} protected Legacy strategies and {} Zapret2 category groups",
        legacy.strategies().len(),
        zapret2.strategies().len()
    );
    Ok(())
}
