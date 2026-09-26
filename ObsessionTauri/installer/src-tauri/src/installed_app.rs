//! Завершение приложения и его прокси только из заменяемой папки установки.

use std::ffi::OsString;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use windows::core::{HRESULT, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
    PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

const STOP_TIMEOUT_MS: u32 = 10_000;

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // Дескриптор получен успешным Win32-вызовом и закрывается один раз.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn image_key(path: &Path) -> String {
    path.to_string_lossy()
        .strip_prefix(r"\\?\")
        .unwrap_or(&path.to_string_lossy())
        .to_lowercase()
}

fn open_process(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Result<Option<OwnedHandle>, String> {
    // PID взят из снимка; исчезнувший процесс уже не мешает обновлению.
    match unsafe { OpenProcess(access, false, pid) } {
        Ok(handle) => Ok(Some(OwnedHandle(handle))),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) => Ok(None),
        Err(error) => Err(format!(
            "could not inspect running Obsession process {pid}: {error}"
        )),
    }
}

fn process_image(process: &OwnedHandle, pid: u32) -> Result<Option<PathBuf>, String> {
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    // Буфер принадлежит этому вызову; length содержит его размер в UTF-16 символах.
    let result = unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    match result {
        Ok(()) => Ok(Some(PathBuf::from(OsString::from_wide(
            &buffer[..length as usize],
        )))),
        Err(_) if unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 => Ok(None),
        Err(error) => Err(format!(
            "could not verify running Obsession process {pid}: {error}"
        )),
    }
}

fn application_pids() -> Result<Vec<u32>, String> {
    // Снимок ограничивает перечисление состоянием процессов на момент вызова.
    let snapshot = OwnedHandle(
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
            .map_err(|error| format!("could not enumerate application processes: {error}"))?,
    );
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut next = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    let mut pids = Vec::new();
    loop {
        match next {
            Ok(()) => {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                if name.eq_ignore_ascii_case("obsession.exe")
                    || name.eq_ignore_ascii_case("obsession-tg-proxy.exe")
                {
                    pids.push(entry.th32ProcessID);
                }
            }
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => break,
            Err(error) => {
                return Err(format!(
                    "could not enumerate application processes: {error}"
                ))
            }
        }
        next = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    Ok(pids)
}

pub(crate) fn stop_installed_application(expected: &Path) -> Result<usize, String> {
    if !expected.is_absolute()
        || !expected
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("obsession.exe"))
    {
        return Err("invalid installed application path".into());
    }
    let expected_key = image_key(expected);
    let proxy_key = image_key(
        &expected
            .parent()
            .ok_or("missing installation directory")?
            .join("bin")
            .join("obsession-tg-proxy.exe"),
    );
    let is_owned = |path: &Path| {
        let actual_key = image_key(path);
        actual_key == expected_key || actual_key == proxy_key
    };
    let mut stopped = 0;
    for pid in application_pids()? {
        // Для чужой копии даже право завершения не запрашивается.
        let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
        let Some(inspect) = open_process(pid, access)? else {
            continue;
        };
        let Some(actual) = process_image(&inspect, pid)? else {
            continue;
        };
        if !is_owned(&actual) {
            continue;
        }
        let Some(process) = open_process(pid, access | PROCESS_TERMINATE)? else {
            continue;
        };
        // Проверка и завершение используют один дескриптор: повторное использование
        // PID между открытиями не даёт права завершить другой процесс.
        let Some(actual) = process_image(&process, pid)? else {
            continue;
        };
        if !is_owned(&actual) {
            continue;
        }
        // WM_CLOSE может скрыть окно в трей. TerminateProcess не завершает
        // дочерний прокси, поэтому его точный путь проверяется отдельно,
        // в том числе когда основное приложение уже завершилось.
        if let Err(error) = unsafe { TerminateProcess(process.0, 0) } {
            if unsafe { WaitForSingleObject(process.0, 0) } != WAIT_OBJECT_0 {
                return Err(format!(
                    "could not stop installed Obsession process {pid}: {error}"
                ));
            }
        }
        if unsafe { WaitForSingleObject(process.0, STOP_TIMEOUT_MS) } != WAIT_OBJECT_0 {
            return Err(format!(
                "installed Obsession process {pid} did not exit before file replacement"
            ));
        }
        stopped += 1;
    }
    Ok(stopped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct Fixture {
        child: Child,
        executable: PathBuf,
    }

    impl Fixture {
        fn spawn_proxy_in(root: &Path) -> Self {
            let bin = root.join("bin");
            fs::create_dir(&bin).unwrap();
            let executable = bin.join("obsession-tg-proxy.exe");
            fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
            let child = Command::new(&executable)
                .args([
                    "--ignored",
                    "--exact",
                    "installed_app::tests::process_fixture",
                ])
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            Self { child, executable }
        }

        fn spawn(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "obsession-stop-test-{}-{label}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&dir).unwrap();
            let executable = dir.join("obsession.exe");
            fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
            let child = Command::new(&executable)
                .args([
                    "--ignored",
                    "--exact",
                    "installed_app::tests::process_fixture",
                ])
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            Self { child, executable }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = fs::remove_file(&self.executable);
            let _ = fs::remove_file(self.executable.with_extension("retired"));
            let _ = fs::remove_dir(self.executable.parent().unwrap());
        }
    }

    #[test]
    #[ignore = "Дочерний процесс для проверки остановки; запускается только родительским тестом"]
    fn process_fixture() {
        std::thread::sleep(Duration::from_secs(60));
    }

    #[test]
    fn stops_owned_process_releases_image_and_preserves_other_copy() {
        let mut owned = Fixture::spawn("owned");
        let mut other = Fixture::spawn("other");
        assert!(owned.child.try_wait().unwrap().is_none());
        assert!(other.child.try_wait().unwrap().is_none());
        assert_eq!(stop_installed_application(&owned.executable).unwrap(), 1);
        assert!(owned.child.try_wait().unwrap().is_some());
        assert!(other.child.try_wait().unwrap().is_none());
        fs::rename(
            &owned.executable,
            owned.executable.with_extension("retired"),
        )
        .unwrap();
        assert_eq!(stop_installed_application(&owned.executable).unwrap(), 0);
    }

    #[test]
    fn windows_prefix_and_case_match_but_other_directories_do_not() {
        assert_eq!(
            image_key(Path::new(r"\\?\C:\Program Files\Obsession\obsession.exe")),
            image_key(Path::new(r"C:\PROGRAM FILES\Obsession\Obsession.exe"))
        );
        assert_ne!(
            image_key(Path::new(r"C:\Program Files\Obsession\obsession.exe")),
            image_key(Path::new(r"C:\Other\Obsession\obsession.exe"))
        );
        assert!(stop_installed_application(Path::new("obsession.exe")).is_err());
    }

    #[test]
    fn stops_orphaned_installed_proxy_and_preserves_other_installation() {
        let mut owned = Fixture::spawn("proxy-owner");
        let other = Fixture::spawn("other-proxy-owner");
        let mut other_proxy = Fixture::spawn_proxy_in(other.executable.parent().unwrap());
        let mut owned_proxy = Fixture::spawn_proxy_in(owned.executable.parent().unwrap());
        owned.child.kill().unwrap();
        owned.child.wait().unwrap();

        assert_eq!(stop_installed_application(&owned.executable).unwrap(), 1);
        assert!(owned_proxy.child.try_wait().unwrap().is_some());
        assert!(other_proxy.child.try_wait().unwrap().is_none());
        fs::rename(
            &owned_proxy.executable,
            owned_proxy.executable.with_extension("retired"),
        )
        .unwrap();
        assert_eq!(stop_installed_application(&owned.executable).unwrap(), 0);
    }
}
