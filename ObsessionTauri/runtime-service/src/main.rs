use std::process::ExitCode;

fn main() -> ExitCode {
    match obsession_runtime_service::scm::run_dispatcher() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // A direct interactive launch reaches this branch because only the
            // SCM can connect a process to the service dispatcher. The
            // installer still does not register the service. A manual SCM
            // registration remains fail-closed unless the protected install
            // and state preflight can construct the production backend.
            eprintln!("Obsession Runtime must be started by Windows SCM: {error}");
            ExitCode::FAILURE
        }
    }
}
