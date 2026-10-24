use std::process::Command;

/// Each integration test owns copied binaries, data/discovery and its host.
pub struct DesktopInstance {
    pub dir: tempfile::TempDir,
    pub pid: Option<u32>,
}
impl DesktopInstance {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("installed app with spaces");
        std::fs::create_dir(&bin).unwrap();
        for (source, name) in [
            (env!("CARGO_BIN_EXE_napomni"), "napomni.exe"),
            (env!("CARGO_BIN_EXE_napomni-cli"), "napomni-cli.exe"),
        ] {
            std::fs::copy(source, bin.join(name)).unwrap();
        }
        Self { dir, pid: None }
    }
    pub fn command(&self) -> Command {
        let mut command = Command::new(
            self.dir
                .path()
                .join("installed app with spaces/napomni-cli.exe"),
        );
        command
            .env("NAPOMNI_DATA_DIR", self.dir.path())
            .env_remove("NAPOMNI_APP_EXE");
        command
    }
    pub fn stop(&mut self) -> anyhow::Result<()> {
        use windows::Win32::{
            Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
            System::Threading::{
                OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
                WaitForSingleObject,
            },
        };
        let Some(pid) = self.pid.take() else {
            return Ok(());
        };
        unsafe {
            let process = match OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) {
                Ok(process) => process,
                Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            let result = (|| -> anyhow::Result<()> {
                if WaitForSingleObject(process, 0) != WAIT_OBJECT_0 {
                    TerminateProcess(process, 1)?;
                }
                anyhow::ensure!(
                    WaitForSingleObject(process, 5000) == WAIT_OBJECT_0,
                    "Test host did not terminate"
                );
                Ok(())
            })();
            let _ = CloseHandle(process);
            result
        }
    }
}
impl Drop for DesktopInstance {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("Failed to stop isolated test host: {error:#}");
        }
    }
}
