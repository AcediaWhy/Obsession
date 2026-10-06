use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use obsession_runtime_protocol::{
    DpiEngine, DpiRuntimeOptions, DpiSelection, DpiStartRequest,
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
    let mut deadline = Instant::now() + Duration::from_secs(15);
    let mut initialization_retries = 0;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            if status.code().map(|code| code as u32) == Some(0xC0000142)
                && initialization_retries == 0
            {
                initialization_retries = 1;
                eprintln!(
                    "parser initialization failed with 0xC0000142; retrying ONCE: {}",
                    launch.executable().display()
                );
                std::thread::sleep(Duration::from_millis(150));
                child = command.spawn()?;
                deadline = Instant::now() + Duration::from_secs(15);
                continue;
            }
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
            "parser verification failed for {} ({status}), cwd={}, args={:?}: {output}",
            launch.executable().display(),
            launch.working_directory().display(),
            launch.arguments()
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
    for strategy in zapret2.strategies() {
        for level in 0..=4 {
            for clienthello in [false, true].into_iter().take(
                if strategy.category() == obsession_runtime_protocol::DpiCategory::Discord { 2 } else { 1 }
            ) {
            let plan = catalog.resolve_dpi_plan(&DpiStartRequest {
                engine: DpiEngine::Zapret2,
                selections: vec![DpiSelection {
                    category: strategy.category(),
                    strategy_id: strategy.strategy_id().to_owned(),
                }],
                options: DpiRuntimeOptions {
                    zapret2_level: level, zapret2_clienthello: clienthello, zapret2_profiles: Vec::new(),
                    legacy_reliability: false,
                    zapret2_overrides: Vec::new(),
                    zapret2_diagnostics: false,
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
            }
        }
    }
    let mut explicit_profile_checks = 0usize;
    for strategy in zapret2.strategies() {
        let category = match strategy.category() {
            obsession_runtime_protocol::DpiCategory::Discord => "discord",
            obsession_runtime_protocol::DpiCategory::YoutubeTwitch => "youtube_twitch",
            obsession_runtime_protocol::DpiCategory::Gaming => "gaming",
            _ => return Err("unsupported Zapret2 category group".into()),
        };
        let manifest: serde_json::Value = serde_json::from_slice(&fs::read(strategy.artifact_resource().path())?)?;
        let choices = ["strategies", "presets", "experiments"].into_iter()
            .flat_map(|key| manifest.get(key).and_then(serde_json::Value::as_array).into_iter().flatten())
            .filter(|profile| profile.get("category").and_then(serde_json::Value::as_str) == Some(category));
        for profile in choices {
            let l7 = profile.get("filter_l7").and_then(serde_json::Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
            let has = |value: &str| l7.iter().any(|entry| entry.as_str() == Some(value));
            let ipset = profile.get("ipset").and_then(serde_json::Value::as_str).is_some();
            let tcp = profile.get("filter_tcp").and_then(serde_json::Value::as_str).is_some();
            let udp = profile.get("filter_udp").and_then(serde_json::Value::as_str).is_some();
            let transport = if !ipset && has("tls") && !udp { "tls" }
                else if !ipset && has("quic") && !tcp { "quic" }
                else if category == "discord" && udp && !tcp && (has("discord") || has("stun")) { "voice" }
                else if has("http") && !udp { "http" }
                else if udp && !tcp && l7.is_empty() { "udp" }
                else { continue; };
            let profile_id = profile.get("id").and_then(serde_json::Value::as_str).ok_or("invalid fixed profile ID")?;
            let plan = catalog.resolve_dpi_plan(&DpiStartRequest {
                engine: DpiEngine::Zapret2,
                selections: vec![DpiSelection { category: strategy.category(), strategy_id: strategy.strategy_id().to_owned() }],
                options: DpiRuntimeOptions {
                    zapret2_level: 0, zapret2_clienthello: false, legacy_reliability: false,
                    zapret2_overrides: Vec::new(), zapret2_diagnostics: false,
                    zapret2_profiles: vec![obsession_runtime_protocol::Zapret2ProfileSelection {
                        category: category.into(), transport: transport.into(), profile_id: profile_id.into(),
                    }],
                },
            })?;
            let materialized = state.materialize(&plan, generation)?;
            let [launch] = materialized.launches() else { return Err("fixed profile produced an unexpected launch count".into()); };
            materialized.reverify()?;
            if dry_run_engines { verify_engine_parser(launch)?; parser_checks += 1; }
            state.cleanup_generation(&materialized)?;
            generation = generation.checked_add(1).ok_or("generation overflow")?;
            explicit_profile_checks += 1;
        }
    }
    println!("verified {explicit_profile_checks} explicit Zapret2 profile selections");
    // Exercise the actual privileged compiler/materializer, not app-side argv.
    // Parser checks use --dry-run exclusively: no WinDivert capture is started.
    let discord = zapret2.strategies().iter().find(|entry| entry.category() == obsession_runtime_protocol::DpiCategory::Discord)
        .ok_or("missing Discord strategy group")?;
    let mut composition_receipts = Vec::new();
    for (api, cdn) in [("research_discord_tls_hostfake_md5", "classic_discord_tls_split_sni"),
        ("classic_discord_tls_split_sni", "research_discord_tls_hostfake_md5")] {
        use obsession_runtime_protocol::zapret2_composition::{DiscordComposition, DiscordScope};
        let pair = DiscordComposition::new(api, cdn)?;
        for diagnostics in [false,true] {
        let plan = catalog.resolve_dpi_plan(&DpiStartRequest {
            engine:DpiEngine::Zapret2,
            selections:vec![DpiSelection{category:discord.category(), strategy_id:discord.strategy_id().into()}],
            options:DpiRuntimeOptions { zapret2_profiles:vec![obsession_runtime_protocol::Zapret2ProfileSelection {
                category:"discord".into(),transport:"tls".into(),profile_id:pair.id()
            }], zapret2_level:4, zapret2_clienthello:false, legacy_reliability:false,
                zapret2_overrides:Vec::new(), zapret2_diagnostics:diagnostics }
        })?;
        let materialized=state.materialize(&plan,generation)?;
        materialized.reverify()?;
        let [launch]=materialized.launches() else { return Err("composition must use one process".into()); };
        let blocks:Vec<_>=launch.arguments().split(|arg|arg=="--new").collect();
        let api_block=blocks.iter().find(|block|block.contains(&format!("--name={api}"))).ok_or("missing API profile")?;
        let cdn_block=blocks.iter().find(|block|block.contains(&format!("--name={cdn}"))).ok_or("missing CDN profile")?;
        if !api_block.contains(&DiscordScope::Api.arguments()[0]) || !api_block.iter().any(|arg|arg.starts_with("--hostlist="))
            || !cdn_block.contains(&DiscordScope::Cdn.arguments()[0]) || cdn_block.iter().any(|arg|arg.starts_with("--hostlist=")) {
            return Err("composition host scopes overlap or lost their lists".into());
        }
        let parser = dry_run_engines || std::env::args().any(|arg|arg=="--dry-run-compositions");
        if diagnostics && [api_block,cdn_block].iter().any(|block| !block.iter()
            .any(|arg|arg.starts_with("--lua-desync=obsession_trace:profile=") && arg.ends_with(":routing"))) {
            return Err("missing exact-flow routing observer".into());
        }
        if parser { verify_engine_parser(launch)?; parser_checks+=1; }
        composition_receipts.push(serde_json::json!({"id":pair.id(),"composition":pair,"diagnostics":diagnostics,
            "args":launch.arguments(),"parserVerified":parser,"packetCaptureStarted":false,"providerTested":false}));
        state.cleanup_generation(&materialized)?; generation+=1;
        }
    }
    if let Some(path)=std::env::args().find_map(|arg|arg.strip_prefix("--composition-receipt=").map(str::to_owned)) {
        fs::write(path,serde_json::to_vec_pretty(&composition_receipts)?)?;
    }
    println!("verified {} Discord compositions with independent API/CDN profiles",composition_receipts.len());
    for strategy in legacy.strategies() {
        let plan = catalog.resolve_dpi_plan(&DpiStartRequest {
            engine: DpiEngine::Legacy,
            selections: vec![DpiSelection {
                category: strategy.category(),
                strategy_id: strategy.strategy_id().to_owned(),
            }],
            options: DpiRuntimeOptions {
                zapret2_level: 0, zapret2_clienthello: false, zapret2_profiles: Vec::new(),
                legacy_reliability: true,
                zapret2_overrides: Vec::new(),
                zapret2_diagnostics: false,
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
                zapret2_level: 0, zapret2_clienthello: false, zapret2_profiles: Vec::new(),
                legacy_reliability: true,
                zapret2_overrides: vec![],
                zapret2_diagnostics: false,
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
