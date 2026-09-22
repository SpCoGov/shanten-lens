use std::{
    backtrace::Backtrace,
    io::Write,
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
    sync::OnceLock,
};
use tauri::{AppHandle, Emitter};

static FIRST_PANIC: OnceLock<String> = OnceLock::new();

pub fn install(app: AppHandle, log_path: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let report = format!("{info}\n\n{}", Backtrace::force_capture());
        // Keep the original failure rather than subsequent poisoned-lock panics.
        if FIRST_PANIC.set(report.clone()).is_ok() {
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                let _ = writeln!(file, "{report}");
            }
            let _ = app.emit("backend:panic", &report);
        }
        previous(info);
    }));
}

#[tauri::command]
pub fn get_backend_panic() -> Option<String> {
    FIRST_PANIC.get().cloned()
}

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

pub fn guard(
    handler: impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke| {
        let resolver = invoke.resolver.clone();
        match catch_command(|| handler(invoke)) {
            Ok(handled) => handled,
            Err(error) => {
                resolver.reject(error);
                true
            }
        }
    }
}

fn catch_command<T>(command: impl FnOnce() -> T) -> Result<T, String> {
    // A failed command is abandoned; the error UI requires a process restart.
    catch_unwind(AssertUnwindSafe(command)).map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "Rust panic".into())
    })
}

#[cfg(test)]
mod tests {
    use super::catch_command;

    #[test]
    fn command_panic_is_caught_before_leaving_the_ipc_handler() {
        assert_eq!(catch_command(|| 42), Ok(42));
        assert_eq!(
            catch_command(|| panic!("no reactor running")),
            Err::<(), _>("no reactor running".into())
        );
    }
}
