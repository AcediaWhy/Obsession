use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use obsession_runtime_protocol::{
    DpiEngine, DpiRuntimeOptions, DpiSelection, DpiStartRequest, Zapret2AdaptiveFunction,
    Zapret2AdaptiveOverride, Zapret2AdaptivePayload, Zapret2AdaptiveRange, Zapret2AdaptiveStep,
    Zapret2AdaptiveTransport, Zapret2AdaptiveValue, ZAPRET2_ADAPTIVE_SCHEMA_VERSION,
};
use obsession_runtime_service::dpi_materializer::{
    MaterializedLaunch, ProtectedDataLayout, RUNTIME_STATE_RELATIVE,
};
use obsession_runtime_service::protected_layout::ProtectedLayout;

/// Validate the shipped executable's parser without loading a packet driver.
fn verify_engine_parser(launch: &MaterializedLaunch) -> Result<(), Box<dyn std::error::Error>> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let log_root = launch
        .response_file()
        .and_then(|path| path.parent())
        .unwrap_or(launch.working_directory());
    let log_path = log_root.join("parser-verification.log");
    let log = fs::File::create(&log_path)?;
    let mut command = Command::new(launch.executable());
    command
        .current_dir(launch.working_directory())
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    if let Some(response_file) = launch.response_file() {
        // @file replaces argv in winws. Put --dry-run INSIDE a separate file;
        // appending it after @file would silently lose the safety option.
        let dry_file = log_root.join("parser-verification.conf");
        fs::write(
            &dry_file,
            format!("--dry-run\n{}", fs::read_to_string(response_file)?),
        )?;
        command.arg(format!("@{}", dry_file.display()));
    } else {
        command.arg("--dry-run").args(launch.arguments());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // The executable requests elevation for normal capture. Parser-only
        // checks need none; this affects only the child and never grants rights.
        command
            .env("__COMPAT_LAYER", "RunAsInvoker")
            .creation_flags(0x08000000);
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("parser timed out: {}", launch.executable().display()).into());
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let output = fs::read_to_string(log_path)?;
    if !status.success() || !output.contains("command line parameters verified") {
        return Err(format!(
            "parser verification failed for {} ({status}): {output}",
            launch.executable().display()
        )
        .into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dry_run_engines = std::env::args().any(|arg| arg == "--dry-run-engines");
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
    let mut parser_checks = 0usize;
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
        if dry_run_engines {
            verify_engine_parser(&materialized.launches()[0])?;
            parser_checks += 1;
        }
        state.cleanup_generation(&materialized)?;
        generation = generation.checked_add(1).ok_or("generation overflow")?;
    }

    // Real combinations matter: all individual files can parse correctly while
    // overlapping capture handles or merged profile boundaries remain broken.
    let mut combined_selections = Vec::new();
    for discord in legacy
        .strategies()
        .iter()
        .filter(|s| s.category() == obsession_runtime_protocol::DpiCategory::Discord)
    {
        for youtube in legacy
            .strategies()
            .iter()
            .filter(|s| s.category() == obsession_runtime_protocol::DpiCategory::YoutubeTwitch)
        {
            combined_selections.push(vec![
                DpiSelection {
                    category: discord.category(),
                    strategy_id: discord.strategy_id().into(),
                },
                DpiSelection {
                    category: youtube.category(),
                    strategy_id: youtube.strategy_id().into(),
                },
            ]);
        }
    }
    let mut baseline = BTreeMap::new();
    for strategy in legacy.strategies() {
        baseline.entry(strategy.category()).or_insert(DpiSelection {
            category: strategy.category(),
            strategy_id: strategy.strategy_id().into(),
        });
    }
    for strategy in legacy.strategies() {
        let mut all = baseline.clone();
        all.insert(
            strategy.category(),
            DpiSelection {
                category: strategy.category(),
                strategy_id: strategy.strategy_id().into(),
            },
        );
        combined_selections.push(all.into_values().rev().collect()); // caller order must not affect routing
    }
    for selections in &combined_selections {
        let plan = catalog.resolve_dpi_plan(&DpiStartRequest {
            engine: DpiEngine::Legacy,
            selections: selections.clone(),
            options: DpiRuntimeOptions {
                zapret2_level: 0,
                legacy_reliability: true,
                zapret2_overrides: vec![],
            },
        })?;
        let materialized = state.materialize(&plan, generation)?;
        let [launch] = materialized.launches() else {
            return Err("Combined Legacy must have exactly ONE process".into());
        };
        materialized.reverify()?;
        if dry_run_engines {
            verify_engine_parser(launch)?;
            parser_checks += 1;
        }
        state.cleanup_generation(&materialized)?;
        generation = generation.checked_add(1).ok_or("generation overflow")?;
    }
    println!(
        "verified {} combined Legacy plans (Discord/YouTube pairs and five-category packs)",
        combined_selections.len()
    );

    for strategy in zapret2.strategies() {
        for level in 0..=3 {
            let plan = catalog.resolve_dpi_plan(&DpiStartRequest {
                engine: DpiEngine::Zapret2,
                selections: vec![DpiSelection {
                    category: strategy.category(),
                    strategy_id: strategy.strategy_id().to_owned(),
                }],
                options: DpiRuntimeOptions {
                    zapret2_level: level,
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
            if dry_run_engines {
                verify_engine_parser(launch)?;
                parser_checks += 1;
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
                    zapret2_level: level,
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
            if dry_run_engines {
                verify_engine_parser(launch)?;
                parser_checks += 1;
            }
            state.cleanup_generation(&materialized)?;
            generation = generation.checked_add(1).ok_or("generation overflow")?;
        }
    }

    println!(
        "verified {} protected Legacy strategies and {} Zapret2 category groups",
        legacy.strategies().len(),
        zapret2.strategies().len()
    );
    if dry_run_engines {
        println!("verified {parser_checks} engine invocations with --dry-run (no packet capture)");
    }
    Ok(())
}
