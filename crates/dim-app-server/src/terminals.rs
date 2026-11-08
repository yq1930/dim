//! `terminal.*` management methods: interactive login shells the operator
//! opens beside a Session, or a program such as a dev server that a login
//! shell starts ([web workspace §4.4](../../../docs/design/web-workspace.md#44-terminals)).
//! The process owns them, so they outlive page reloads and a Session's idle
//! unload; unloading the plugin ends every one.
use crate::methods::ManagementMethods;
use dim_agent_capabilities::{
    host_platform::{HostPlatform, ProcessSpec, PtyProcessTree, PtySize},
    methods::{Interruption, MethodDecl, RpcError, params, reply},
};
use dim_agent_runtime::Names;
use rutis::Ctx;
use rutis::host::{BoxFuture, CtxHost, DynPlugin, Effect, Key, TypeKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    ffi::OsString,
    io::Read as _,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

const NAMESPACE: &str = "terminal";
const OPEN: &str = "terminal.open";
const LIST: &str = "terminal.list";
const READ: &str = "terminal.read";
const WRITE: &str = "terminal.write";
const RESIZE: &str = "terminal.resize";
const CLOSE: &str = "terminal.close";
/// The output a terminal keeps for a reader that attaches later.
const RETAINED: usize = 256 * 1024;
const MAX_TERMINALS: usize = 32;
const MAX_TITLE: usize = 100;
const MAX_WAIT: Duration = Duration::from_secs(25);
const REAP_POLL: Duration = Duration::from_millis(200);
const OUTPUT_GRACE: Duration = Duration::from_secs(1);
/// Variables describing the service's own terminal, which this one replaces.
const REPLACED_ENV: [&str; 8] = [
    "TERM",
    "COLORTERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "ITERM_SESSION_ID",
    "PWD",
    "OLDPWD",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpenRequest {
    owner: String,
    cwd: String,
    cols: u16,
    rows: u16,
    /// A program and its arguments to run in place of the interactive
    /// shell; the login shell starts it, so it sees the operator's
    /// environment, and the terminal exits with it.
    #[serde(default)]
    command: Option<Vec<String>>,
    /// The title listed; the program's or the shell's name by default.
    #[serde(default)]
    title: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListRequest {
    owner: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadRequest {
    terminal_id: String,
    cursor: u64,
    #[serde(default)]
    wait_ms: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WriteRequest {
    terminal_id: String,
    data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResizeRequest {
    terminal_id: String,
    cols: u16,
    rows: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CloseRequest {
    terminal_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Chunk {
    data: String,
    cursor: u64,
    reset: bool,
    exited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<u32>,
}

#[derive(Default)]
struct Output {
    data: Vec<u8>,
    /// Offset of `data[0]` in everything the shell wrote.
    start: u64,
    ended: bool,
    exit: Option<Option<u32>>,
}

impl Output {
    fn push(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
        if self.data.len() > 2 * RETAINED {
            let dropped = self.data.len() - RETAINED;
            self.data.drain(..dropped);
            self.start += dropped as u64;
        }
    }

    /// The text after `cursor`, the cursor after it, and whether output
    /// before it is no longer kept.
    fn after(&self, cursor: u64) -> (String, u64, bool) {
        let end = self.start + self.data.len() as u64;
        let reset = cursor < self.start || cursor > end;
        let mut from = if reset { self.start } else { cursor };
        let mut bytes = &self.data[(from - self.start) as usize..];
        if reset {
            let partial = bytes
                .iter()
                .take(3)
                .take_while(|byte| **byte & 0xC0 == 0x80)
                .count();
            bytes = &bytes[partial..];
            from += partial as u64;
        }
        if !self.ended {
            bytes = &bytes[..complete(bytes)];
        }
        (
            String::from_utf8_lossy(bytes).into_owned(),
            from + bytes.len() as u64,
            reset,
        )
    }
}

/// The length of `bytes` without a UTF-8 sequence still arriving at its end.
fn complete(bytes: &[u8]) -> usize {
    for back in 1..=bytes.len().min(4) {
        let byte = bytes[bytes.len() - back];
        if byte & 0xC0 == 0x80 {
            continue;
        }
        let width = match byte {
            0xF0.. => 4,
            0xE0.. => 3,
            0xC0.. => 2,
            _ => 1,
        };
        return if back < width {
            bytes.len() - back
        } else {
            bytes.len()
        };
    }
    bytes.len()
}

struct Terminal {
    owner: String,
    cwd: String,
    title: String,
    command: Option<Vec<String>>,
    order: u64,
    pty: tokio::sync::Mutex<PtyProcessTree>,
    input: mpsc::Sender<Vec<u8>>,
    output: Arc<Mutex<Output>>,
    changed: Arc<watch::Sender<()>>,
    closed: CancellationToken,
}

impl Terminal {
    fn finish(&self, code: Option<u32>) {
        self.output.lock().unwrap().exit.get_or_insert(code);
        self.changed.send_replace(());
    }

    async fn end(&self) {
        self.closed.cancel();
        let _ = self.pty.lock().await.terminate().await;
        self.finish(None);
    }
}

#[derive(Default)]
struct Terminals {
    open: Mutex<HashMap<String, Arc<Terminal>>>,
    opened: AtomicU64,
    tasks: TaskTracker,
}

fn unknown() -> RpcError {
    RpcError::app(-32000, "terminal_unknown", "The terminal is not open")
}

fn failed(error: impl ToString) -> RpcError {
    RpcError::app(-32000, "terminal_failed", error.to_string())
}

fn size(cols: u16, rows: u16) -> Result<PtySize, RpcError> {
    if !(2..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
        return Err(RpcError::invalid(
            "cols must be 2 to 1000 and rows 1 to 1000",
        ));
    }
    Ok(PtySize { cols, rows })
}

impl Terminals {
    fn get(&self, id: &str) -> Result<Arc<Terminal>, RpcError> {
        self.open
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(unknown)
    }

    async fn open(&self, platform: &HostPlatform, request: OpenRequest) -> Result<Value, RpcError> {
        if request.owner.is_empty() {
            return Err(RpcError::invalid("owner must not be empty"));
        }
        let size = size(request.cols, request.rows)?;
        let cwd = PathBuf::from(&request.cwd);
        if !cwd.is_absolute()
            || !tokio::fs::metadata(&cwd)
                .await
                .is_ok_and(|meta| meta.is_dir())
        {
            return Err(RpcError::invalid(
                "cwd must be an existing absolute directory",
            ));
        }
        if request
            .command
            .as_ref()
            .is_some_and(|command| command.first().is_none_or(|program| program.is_empty()))
        {
            return Err(RpcError::invalid("command must name a program"));
        }
        if request
            .title
            .as_ref()
            .is_some_and(|title| title.is_empty() || title.chars().count() > MAX_TITLE)
        {
            return Err(RpcError::invalid("title must be 1 to 100 characters"));
        }
        if self.open.lock().unwrap().len() >= MAX_TERMINALS {
            return Err(RpcError::app(
                -32000,
                "terminal_limit",
                "Too many terminals are open",
            ));
        }
        let (executable, mut args) = login_shell(&platform.env);
        let started = request
            .command
            .as_ref()
            .map(|command| PathBuf::from(&command[0]));
        let title = request.title.clone().unwrap_or_else(|| {
            started
                .as_deref()
                .unwrap_or(&executable)
                .file_stem()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        if let Some(command) = &request.command {
            args = program_args(command);
        }
        let mut pty = platform
            .spawn_pty(
                ProcessSpec {
                    executable,
                    args,
                    cwd,
                    env: terminal_env(&platform.env),
                },
                size,
            )
            .map_err(failed)?;
        let reader = pty.take_reader();
        let input = spawn_writer(pty.take_writer());
        let terminal = Arc::new(Terminal {
            owner: request.owner,
            cwd: request.cwd,
            title,
            command: request.command,
            order: self.opened.fetch_add(1, Ordering::Relaxed),
            pty: tokio::sync::Mutex::new(pty),
            input,
            output: Arc::default(),
            changed: Arc::new(watch::Sender::new(())),
            closed: CancellationToken::new(),
        });
        spawn_reader(reader, terminal.output.clone(), terminal.changed.clone());
        self.tasks.spawn(reap(terminal.clone()));
        let id = uuid::Uuid::new_v4().to_string();
        self.open
            .lock()
            .unwrap()
            .insert(id.clone(), terminal.clone());
        reply(json!({"terminalId": id, "cwd": terminal.cwd, "title": terminal.title}))
    }

    fn list(&self, owner: &str) -> Result<Value, RpcError> {
        let open = self.open.lock().unwrap();
        let mut owned: Vec<_> = open
            .iter()
            .filter(|(_, terminal)| terminal.owner == owner)
            .collect();
        owned.sort_by_key(|(_, terminal)| terminal.order);
        let terminals: Vec<_> = owned
            .into_iter()
            .map(|(id, terminal)| {
                let mut listed = json!({
                    "terminalId": id,
                    "cwd": terminal.cwd,
                    "title": terminal.title,
                    "exited": terminal.output.lock().unwrap().exit.is_some(),
                });
                if let Some(command) = &terminal.command {
                    listed["command"] = json!(command);
                }
                listed
            })
            .collect();
        reply(json!({ "terminals": terminals }))
    }

    async fn read(
        &self,
        request: ReadRequest,
        stopping: CancellationToken,
    ) -> Result<Value, RpcError> {
        let terminal = self.get(&request.terminal_id)?;
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(request.wait_ms).min(MAX_WAIT);
        let mut changes = terminal.changed.subscribe();
        loop {
            let (chunk, waiting) = {
                let output = terminal.output.lock().unwrap();
                let (data, cursor, reset) = output.after(request.cursor);
                let waiting = data.is_empty() && !reset && output.exit.is_none();
                let chunk = Chunk {
                    data,
                    cursor,
                    reset,
                    exited: output.exit.is_some(),
                    exit_code: output.exit.flatten(),
                };
                (chunk, waiting)
            };
            if !waiting || tokio::time::Instant::now() >= deadline {
                return reply(chunk);
            }
            tokio::select! {
                _ = changes.changed() => {}
                () = tokio::time::sleep_until(deadline) => {}
                () = stopping.cancelled() => return Err(RpcError::interrupted()),
            }
        }
    }

    fn write(&self, request: WriteRequest) -> Result<Value, RpcError> {
        self.get(&request.terminal_id)?
            .input
            .send(request.data.into_bytes())
            .map_err(|_| failed("The terminal no longer takes input"))?;
        reply(json!({}))
    }

    async fn resize(&self, request: ResizeRequest) -> Result<Value, RpcError> {
        let size = size(request.cols, request.rows)?;
        let terminal = self.get(&request.terminal_id)?;
        terminal.pty.lock().await.resize(size).map_err(failed)?;
        reply(json!({}))
    }

    async fn close(&self, id: &str) -> Result<Value, RpcError> {
        let terminal = self.open.lock().unwrap().remove(id).ok_or_else(unknown)?;
        terminal.end().await;
        reply(json!({}))
    }

    async fn close_all(&self) {
        let open: Vec<_> = self
            .open
            .lock()
            .unwrap()
            .drain()
            .map(|(_, terminal)| terminal)
            .collect();
        for terminal in open {
            terminal.end().await;
        }
        self.tasks.close();
        self.tasks.wait().await;
    }
}

fn spawn_reader(
    mut reader: Box<dyn std::io::Read + Send>,
    output: Arc<Mutex<Output>>,
    changed: Arc<watch::Sender<()>>,
) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => output.lock().unwrap().push(&buffer[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
            changed.send_replace(());
        }
        output.lock().unwrap().ended = true;
        changed.send_replace(());
    });
}

/// Writes input in order on its own thread, so a shell that stops reading
/// never blocks a call or the terminal's other operations.
fn spawn_writer(mut writer: Box<dyn std::io::Write + Send>) -> mpsc::Sender<Vec<u8>> {
    let (input, received) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for bytes in received {
            if writer
                .write_all(&bytes)
                .and_then(|()| writer.flush())
                .is_err()
            {
                break;
            }
        }
    });
    input
}

/// Records the shell's exit once it is reaped and its output has ended, or
/// a moment after when a background job still holds the terminal open.
async fn reap(terminal: Arc<Terminal>) {
    let code = loop {
        tokio::select! {
            () = terminal.closed.cancelled() => return,
            () = tokio::time::sleep(REAP_POLL) => {}
        }
        match terminal.pty.lock().await.try_wait() {
            Ok(None) => continue,
            Ok(Some(status)) => break Some(status.exit_code()),
            Err(_) => break None,
        }
    };
    let mut changes = terminal.changed.subscribe();
    let grace = tokio::time::sleep(OUTPUT_GRACE);
    tokio::pin!(grace);
    while !terminal.output.lock().unwrap().ended {
        tokio::select! {
            _ = changes.changed() => {}
            () = &mut grace => break,
        }
    }
    terminal.finish(code);
}

fn login_shell(env: &[(OsString, OsString)]) -> (PathBuf, Vec<OsString>) {
    if cfg!(windows) {
        let comspec = env
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("COMSPEC"))
            .map(|(_, value)| PathBuf::from(value));
        return (comspec.unwrap_or_else(|| "cmd.exe".into()), Vec::new());
    }
    let shell = env
        .iter()
        .find(|(key, _)| key == "SHELL")
        .map(|(_, value)| PathBuf::from(value))
        .filter(|path| path.is_absolute() && path.is_file())
        .or_else(|| {
            ["/bin/zsh", "/bin/bash"]
                .into_iter()
                .map(PathBuf::from)
                .find(|path| path.is_file())
        })
        .unwrap_or_else(|| "/bin/sh".into());
    (shell, vec!["-l".into()])
}

/// The login shell's arguments that run `command` in its place: `exec "$0"
/// "$@"` keeps every argument as given. Windows hands it to `cmd.exe /C`.
fn program_args(command: &[String]) -> Vec<OsString> {
    let mut args: Vec<OsString> = if cfg!(windows) {
        vec!["/C".into()]
    } else {
        vec!["-l".into(), "-c".into(), r#"exec "$0" "$@""#.into()]
    };
    args.extend(command.iter().map(OsString::from));
    args
}

fn terminal_env(env: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    let mut terminal: Vec<_> = env
        .iter()
        .filter(|(key, _)| !REPLACED_ENV.iter().any(|name| key == name))
        .cloned()
        .collect();
    terminal.push(("TERM".into(), "xterm-256color".into()));
    terminal.push(("COLORTERM".into(), "truecolor".into()));
    if !terminal
        .iter()
        .any(|(key, _)| key == "LANG" || key == "LC_ALL" || key == "LC_CTYPE")
    {
        terminal.push(("LANG".into(), "en_US.UTF-8".into()));
    }
    terminal
}

pub(crate) struct TerminalsPlugin {
    platform: Key<HostPlatform>,
    methods: Key<ManagementMethods>,
}

impl TerminalsPlugin {
    pub(crate) fn new(names: &Names<'_>) -> rutis::Result<Self> {
        Ok(Self {
            platform: names.key()?,
            methods: names.key()?,
        })
    }
}

impl DynPlugin for TerminalsPlugin {
    fn name(&self) -> &str {
        "Terminals"
    }

    fn injects(&self) -> Vec<TypeKey> {
        vec![self.platform.erase(), self.methods.erase()]
    }

    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let platform = ctx.require_at(&self.platform)?;
            let methods = ctx.require_at(&self.methods)?;
            let terminals = Arc::new(Terminals::default());
            let owner = terminals.clone();
            ctx.effect(move || async move {
                owner.close_all().await;
                Ok(())
            })?;
            methods.register(
                ctx,
                NAMESPACE,
                vec![
                    MethodDecl::new(OPEN, true, Interruption::Report),
                    MethodDecl::new(LIST, false, Interruption::Report),
                    MethodDecl::new(READ, false, Interruption::Report),
                    MethodDecl::new(WRITE, true, Interruption::Report),
                    MethodDecl::new(RESIZE, true, Interruption::Report),
                    MethodDecl::new(CLOSE, true, Interruption::Report),
                ],
                Arc::new(move |method, value, stopping, _| {
                    let (terminals, platform) = (terminals.clone(), platform.clone());
                    Box::pin(async move {
                        match method.as_str() {
                            OPEN => terminals.open(&platform, params(value)?).await,
                            LIST => {
                                let ListRequest { owner } = params(value)?;
                                terminals.list(&owner)
                            }
                            READ => terminals.read(params(value)?, stopping).await,
                            WRITE => terminals.write(params(value)?),
                            RESIZE => terminals.resize(params(value)?).await,
                            CLOSE => {
                                let CloseRequest { terminal_id } = params(value)?;
                                terminals.close(&terminal_id).await
                            }
                            _ => unreachable!("registered terminal method"),
                        }
                    })
                }),
            )?;
            Ok(Effect::Done)
        })
    }
}
