//! Current-user Windows IPC. Only the desktop owner loads reminder state.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    path::PathBuf,
    sync::mpsc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions},
};

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_MESSAGE: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub command: String,
    pub arguments: Value,
}
pub struct Pending {
    pub request: Request,
    pub reply: tokio::sync::oneshot::Sender<Value>,
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}
fn directory() -> Result<PathBuf> {
    let path = crate::chat::ChatSession::default_directory();
    std::fs::create_dir_all(&path)?;
    Ok(std::fs::canonicalize(path)?)
}
fn pipe_name() -> Result<String> {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    directory()?
        .to_string_lossy()
        .to_lowercase()
        .hash(&mut hash);
    Ok(format!(r"\\.\pipe\napomni-{:016x}", hash.finish()))
}
/// Held for the entire desktop lifetime, before any mutable state is opened.
pub fn claim_owner() -> Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(directory()?.join("owner.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn create_pipe(name: &str) -> Result<NamedPipeServer> {
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
            },
        },
        core::w,
    };
    // OW means the object's owner, i.e. this Windows user. The protected DACL
    // excludes inherited Everyone/Administrators access; remote clients are rejected.
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("D:P(A;;GA;;;OW)"),
            1,
            &mut descriptor,
            None,
        )?;
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let result = ServerOptions::new()
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            );
        LocalFree(Some(HLOCAL(descriptor.0)));
        Ok(result?)
    }
}
/// Requests are processed on the desktop thread, including window reconciliation.
pub fn serve() -> Result<mpsc::Receiver<Pending>> {
    let name = pipe_name()?;
    let (tx, rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let runtime = runtime()?;
            runtime.block_on(async {
                let mut pipe = create_pipe(&name)?;
                let _ = ready_tx.send(Ok(()));
                loop {
                    pipe.connect().await?;
                    let mut connected = pipe;
                    pipe = create_pipe(&name)?;
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let result = tokio::time::timeout(TIMEOUT, async {
                            let bytes = read_frame(&mut connected).await?;
                            let request: Request = serde_json::from_slice(&bytes)?;
                            let (reply, result) = tokio::sync::oneshot::channel();
                            tx.send(Pending { request, reply })?;
                            let mut result = result.await?;
                            result["host_pid"] = json!(std::process::id());
                            write_frame(&mut connected, &serde_json::to_vec(&result)?).await
                        })
                        .await;
                        if let Ok(Err(error)) = result {
                            eprintln!("Local command failed: {error:#}");
                        }
                    });
                }
                #[allow(unreachable_code)]
                Ok::<(), anyhow::Error>(())
            })
        })();
        if let Err(error) = result {
            let _ = ready_tx.send(Err(format!("{error:#}")));
        }
    });
    ready_rx
        .recv_timeout(TIMEOUT)?
        .map_err(anyhow::Error::msg)?;
    Ok(rx)
}
async fn read_frame(stream: &mut (impl tokio::io::AsyncRead + Unpin)) -> Result<Vec<u8>> {
    let len = stream.read_u32_le().await? as usize;
    ensure!(len <= MAX_MESSAGE, "Command response/request exceeds 1 MiB");
    let mut bytes = vec![0; len];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}
async fn write_frame(
    stream: &mut (impl tokio::io::AsyncWrite + Unpin),
    bytes: &[u8],
) -> Result<()> {
    ensure!(
        bytes.len() <= MAX_MESSAGE,
        "Command response/request exceeds 1 MiB"
    );
    stream.write_u32_le(bytes.len() as u32).await?;
    stream.write_all(bytes).await?;
    Ok(())
}
pub fn request(command: &str, arguments: Value, launch: bool) -> Result<Value> {
    let name = pipe_name()?;
    runtime()?.block_on(async {
        tokio::time::timeout(TIMEOUT, async {
            let mut launched = false;
            let mut pipe = loop {
                match ClientOptions::new().open(&name) {
                    Ok(pipe) => break pipe,
                    Err(error) if matches!(error.raw_os_error(), Some(2 | 231)) => {
                        if launch && !launched && error.raw_os_error() == Some(2) {
                            launch_app()?;
                            launched = true;
                        }
                        tokio::time::sleep(Duration::from_millis(40)).await;
                    }
                    Err(error) => return Err(error.into()),
                }
            };
            write_frame(
                &mut pipe,
                &serde_json::to_vec(&Request {
                    command: command.into(),
                    arguments,
                })?,
            )
            .await?;
            // Once sent, never repeat a command: a missing reply may follow execution.
            let bytes = read_frame(&mut pipe).await.context(
                "Command sent, but its result could not be received; execution is uncertain",
            )?;
            Ok(serde_json::from_slice(&bytes)?)
        })
        .await
        .context(
            "Application did not return a result within 20 seconds; execution may be uncertain",
        )?
    })
}
fn launch_app() -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::{
            Foundation::CloseHandle,
            System::Threading::{
                CREATE_NO_WINDOW, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
            },
        },
        core::{PCWSTR, PWSTR},
    };
    let executable = std::env::var_os("NAPOMNI_APP_EXE")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_exe()?.with_file_name("napomni.exe"));
    let executable_wide = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut command = "napomni --background"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // No handles are inherited, including capture pipes owned by the CLI's caller.
    unsafe {
        CreateProcessW(
            PCWSTR(executable_wide.as_ptr()),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW,
            None,
            None,
            &startup,
            &mut process,
        )
        .with_context(|| format!("Cannot launch application {}", executable.display()))?;
        let _ = CloseHandle(process.hThread);
        let _ = CloseHandle(process.hProcess);
    }
    Ok(())
}
pub fn run_cli(args: Vec<String>) -> Result<()> {
    if args.is_empty() || matches!(args[0].as_str(), "--help" | "-h" | "help") {
        println!(
            "Napomni CLI\n\nUsage: napomni-cli list\n       napomni-cli notify <text>\n\nnotify  Show one independent notification (blue border, sound) for each call, even with identical text. Quote text as one argument; Unicode and newlines are preserved. Returns success/error without waiting for closure.\n\nlist  Return JSON with all saved reminders (stable IDs, schedule descriptions and statuses).\n\nConnects to the current Windows user's local app; starts it in the background if needed.\nSuccess is reported only after the application's result. Errors go to stderr with exit code 1."
        );
        return Ok(());
    }
    let arguments = match args[0].as_str() {
        "list" if args.len() == 1 => json!({}),
        "notify" if args.len() == 2 => json!({"text":args[1]}),
        _ => bail!("Unknown command or arguments. Run napomni-cli --help"),
    };
    let result = request(&args[0], arguments, true)?;
    if result["ok"] != true {
        bail!("{}", result["error"].as_str().unwrap_or("Command failed"));
    }
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
