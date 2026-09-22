#![cfg(windows)]
//! No real process, driver or service is started by these integration tests.
use obsession_runtime_protocol::{
    DpiCategory, DpiEngine, DpiRuntimeOptions, DpiSelection, DpiStartRequest,
};
use obsession_runtime_service::{
    dpi_executor::{
        DpiExecutor, ProcessError, ProcessIdentity, RuntimeProcessGroup, RuntimeProcessLauncher,
    },
    dpi_materializer::{MaterializedLaunch, ProtectedDataLayout, RUNTIME_STATE_RELATIVE},
    protected_layout::{ProtectedLayout, RESOURCE_MANIFEST},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Counts {
    starts: usize,
    stops: usize,
}
struct Launcher(Arc<Mutex<Counts>>);
struct Group {
    counts: Arc<Mutex<Counts>>,
    identities: Vec<ProcessIdentity>,
}
impl RuntimeProcessGroup for Group {
    fn identities(&self) -> &[ProcessIdentity] {
        &self.identities
    }
    fn stop(&mut self, _: Duration) -> Result<(), ProcessError> {
        self.counts.lock().unwrap().stops += 1;
        Ok(())
    }
}
impl RuntimeProcessLauncher for Launcher {
    type Group = Group;
    fn launch_group(
        &self,
        launches: &[MaterializedLaunch],
        _: Duration,
    ) -> Result<Group, ProcessError> {
        assert_eq!(
            launches.len(),
            1,
            "overlapping capture must never use multiple processes"
        );
        let config = fs::read_to_string(launches[0].response_file().unwrap()).unwrap();
        assert_eq!(config.matches("--wf-tcp=").count(), 1);
        assert!(config.contains("discord.com"));
        assert!(config.contains("youtube.com"));
        assert!(config.contains("--dpi-desync=\"multidisorder\""));
        let mut counts = self.0.lock().unwrap();
        counts.starts += 1;
        Ok(Group {
            counts: self.0.clone(),
            identities: vec![ProcessIdentity {
                pid: 100 + counts.starts as u32,
                creation_time_100ns: 500 + counts.starts as u64,
                executable_sha256: launches[0].executable_sha256().into(),
            }],
        })
    }
}

#[test]
fn combined_plan_preserves_selections_integrity_and_exact_stop_restart() {
    let root = Fixture(
        std::env::temp_dir().join(format!(
            "obsession-combined-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )),
    );
    let program_files = root.0.join("Program Files");
    let install = program_files.join("Obsession");
    let program_data = root.0.join("ProgramData");
    let state_root = program_data.join(RUNTIME_STATE_RELATIVE);
    fs::create_dir_all(&state_root).unwrap();
    let resources = [
        ("runtime/legacy/winws.exe", "test engine"),
        ("runtime/configs/discord.conf", "--wf-tcp=443,8443 --filter-tcp=443,8443 --hostlist-domains=discord.com --dpi-desync=multisplit --new"),
        ("runtime/configs/youtube.conf", "--wf-tcp=80,443 --filter-tcp=443 --hostlist-domains=youtube.com --dpi-desync=multidisorder --dpi-desync-split-pos=midsld"),
    ];
    let files: Vec<_> = resources.iter().map(|(path, text)| {
        let destination = install.join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, text).unwrap();
        serde_json::json!({ "path": path, "size": text.len(), "sha256": format!("{:x}", Sha256::digest(text.as_bytes())) })
    }).collect();
    let manifest = serde_json::json!({ "schema_version": 1, "engines": [{ "engine": "legacy", "executable": "runtime/legacy/winws.exe", "files": files,
        "strategies": [
            { "id": "discord_14.conf", "category": "discord", "artifact": "runtime/configs/discord.conf", "dependencies": [] },
            { "id": "youtube_twitch_11.conf", "category": "youtubeTwitch", "artifact": "runtime/configs/youtube.conf", "dependencies": [] }
        ]
    }] });
    fs::create_dir_all(install.join(RESOURCE_MANIFEST).parent().unwrap()).unwrap();
    fs::write(
        install.join(RESOURCE_MANIFEST),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let catalog = ProtectedLayout::inspect(&program_files, &install)
        .unwrap()
        .load_verified_catalog()
        .unwrap();
    let selections = vec![
        DpiSelection {
            category: DpiCategory::YoutubeTwitch,
            strategy_id: "youtube_twitch_11.conf".into(),
        },
        DpiSelection {
            category: DpiCategory::Discord,
            strategy_id: "discord_14.conf".into(),
        },
    ];
    let request = DpiStartRequest {
        engine: DpiEngine::Legacy,
        selections: selections.clone(),
        options: DpiRuntimeOptions {
            zapret2_level: 0,
            legacy_reliability: true,
            zapret2_overrides: vec![],
        },
    };
    let plan = catalog.resolve_dpi_plan(&request).unwrap();
    let state = ProtectedDataLayout::inspect(&program_data, &state_root).unwrap();
    let materialized = state.materialize(&plan, 900).unwrap();
    let rendered = fs::read_to_string(materialized.launches()[0].response_file().unwrap()).unwrap();
    assert!(rendered.find("discord.com") < rendered.find("youtube.com"));
    materialized.reverify().unwrap();
    let mut reverse = request.clone();
    reverse.selections.reverse();
    let reverse = state
        .materialize(&catalog.resolve_dpi_plan(&reverse).unwrap(), 901)
        .unwrap();
    assert_eq!(
        rendered,
        fs::read_to_string(reverse.launches()[0].response_file().unwrap()).unwrap()
    );
    fs::write(
        materialized.launches()[0].response_file().unwrap(),
        "--wf-tcp=1-65535",
    )
    .unwrap();
    assert!(materialized.reverify().is_err());
    state.cleanup_generation(&materialized).unwrap();
    state.cleanup_generation(&reverse).unwrap();
    let counts = Arc::new(Mutex::new(Counts::default()));
    let mut executor = DpiExecutor::new(state.clone(), Launcher(counts.clone()));
    let first = executor.start(&plan).unwrap().generation;
    assert_eq!(executor.snapshot().unwrap().selections, selections);
    assert_eq!(executor.active_processes().len(), 1);
    assert!(executor.stop(first + 1).is_err());
    assert_eq!(executor.snapshot().unwrap().generation, first);
    executor.stop(first).unwrap();
    assert!(executor.active_processes().is_empty());
    assert!(executor.snapshot().is_none());
    assert!(!state
        .root()
        .join("dpi/generations")
        .join(first.to_string())
        .exists());
    let second = executor.start(&plan).unwrap().generation;
    assert_ne!(first, second);
    assert_eq!(executor.snapshot().unwrap().selections, selections);
    assert!(executor.stop(first).is_err());
    executor.stop(second).unwrap();
    assert_eq!(counts.lock().unwrap().starts, 2);
    assert_eq!(counts.lock().unwrap().stops, 2);
}
