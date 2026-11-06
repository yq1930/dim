//! One language server process for a Session workspace: stdio JSON-RPC, the
//! `initialize` handshake, and queries that open their document only for
//! their own request.
use super::{LspError, Operation};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    sync::{oneshot, watch},
    time::{Instant, sleep_until, timeout},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use url::Url;

const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_HEADER_BYTES: u64 = 8 * 1024;
const STDERR_TAIL_BYTES: usize = 2 * 1024;
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);
const CANCEL_GRACE: Duration = Duration::from_secs(2);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// What a server process runs with; a call whose launch differs replaces it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Launch {
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub initialization_options: Option<Value>,
    /// The canonical workspace, the server's only workspace folder.
    pub root: PathBuf,
}

/// One query. The document goes to the server as `text`, so the answer is
/// for the file as it is on disk now.
pub(super) struct Query<'a> {
    pub operation: Operation,
    pub uri: &'a str,
    pub language: &'a str,
    pub text: &'a str,
    /// Zero-based line and UTF-16 character.
    pub position: (u64, u64),
}

pub(super) enum Answer {
    Locations(Vec<Location>),
    Hover(Option<String>),
}

/// The start of a result range, zero-based as the server sent it.
pub(super) struct Location {
    pub uri: String,
    pub line: u64,
    pub character: u64,
}

#[derive(Clone, Default)]
struct State {
    initialized: Option<Result<Initialized, LspError>>,
    report: Option<Report>,
    /// Why the transport ended, such as `exited (exit status: 1): <stderr>`.
    ended: Option<String>,
}

impl State {
    fn settled(&self) -> bool {
        self.ended.is_some()
            || match &self.initialized {
                None => false,
                Some(Err(_)) => true,
                Some(Ok(initialized)) => {
                    !initialized.waits_for_quiescence
                        || self.report.as_ref().is_some_and(|report| report.quiescent)
                }
            }
    }
}

#[derive(Clone)]
struct Initialized {
    capabilities: Value,
    waits_for_quiescence: bool,
}

/// The last `experimental/serverStatus` notification.
#[derive(Clone)]
struct Report {
    quiescent: bool,
    health: String,
    message: Option<String>,
}

enum Fault {
    /// The transport ended; the reason as in [`State::ended`].
    Ended(String),
    /// The server answered with a JSON-RPC error.
    Error(String),
}

type Reply = Result<Value, Fault>;

pub(super) struct Server {
    pub launch: Launch,
    folder: Value,
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    child: tokio::sync::Mutex<Child>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Reply>>>,
    next_id: AtomicU64,
    state: watch::Sender<State>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_done: Mutex<Option<oneshot::Receiver<()>>>,
    /// Held by one query at a time, so documents are opened and closed in turn.
    turn: tokio::sync::Mutex<()>,
    tasks: TaskTracker,
}

impl Server {
    /// Starts the process and its `initialize` handshake; queries wait for it.
    pub fn start(launch: Launch) -> Result<Arc<Self>, LspError> {
        let uri = Url::from_file_path(&launch.root).expect("a canonical workspace is absolute");
        let mut child = Command::new(&launch.command)
            .args(&launch.args)
            .envs(&launch.env)
            .current_dir(&launch.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                LspError::unavailable(if error.kind() == std::io::ErrorKind::NotFound {
                    format!(
                        "{} is not installed: `{}` was not found.",
                        launch.id, launch.command
                    )
                } else {
                    format!(
                        "{} could not start `{}`: {error}.",
                        launch.id, launch.command
                    )
                })
            })?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let (drained, stderr_done) = oneshot::channel();
        let name = launch.root.file_name().map_or_else(
            || uri.to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let server = Arc::new(Self {
            folder: json!({ "uri": uri.as_str(), "name": name }),
            launch,
            stdin: tokio::sync::Mutex::new(Some(stdin)),
            child: tokio::sync::Mutex::new(child),
            pending: Mutex::default(),
            next_id: AtomicU64::new(1),
            state: watch::Sender::new(State::default()),
            stderr: Arc::default(),
            stderr_done: Mutex::new(Some(stderr_done)),
            turn: tokio::sync::Mutex::new(()),
            tasks: TaskTracker::new(),
        });
        server
            .tasks
            .spawn(drain(stderr, server.stderr.clone(), drained));
        server.tasks.spawn(read(Arc::downgrade(&server), stdout));
        server.tasks.spawn(server.clone().initialize());
        Ok(server)
    }

    /// Whether the process ended or never started; the next call replaces it.
    pub fn failed(&self) -> bool {
        let state = self.state.borrow();
        state.ended.is_some() || matches!(state.initialized, Some(Err(_)))
    }

    /// What a server that reports its health says while it is not `ok`.
    pub fn health(&self) -> Option<String> {
        let state = self.state.borrow();
        let report = state.report.as_ref()?;
        let message = report.message.as_deref()?;
        (report.health != "ok").then(|| format!("{}: {message}", self.launch.id))
    }

    pub async fn query(
        &self,
        query: &Query<'_>,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<Answer, LspError> {
        let id = &self.launch.id;
        let _turn = tokio::select! {
            turn = self.turn.lock() => turn,
            () = sleep_until(deadline) => {
                return Err(LspError::timeout(format!("{id} is still busy with an earlier query.")));
            }
            () = cancel.cancelled() => return Err(LspError::cancelled()),
        };
        let initialized = self.ready(deadline, cancel).await?;
        let operation = query.operation;
        if matches!(
            initialized.capabilities.get(operation.capability()),
            None | Some(Value::Null | Value::Bool(false))
        ) {
            return Err(LspError::unsupported(format!(
                "{id} does not support {}.",
                operation.name()
            )));
        }
        let document = json!({ "uri": query.uri });
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": {
                "uri": query.uri,
                "languageId": query.language,
                "version": 1,
                "text": query.text,
            }}),
        )
        .await
        .map_err(|fault| self.failure(fault))?;
        let mut params = json!({
            "textDocument": document,
            "position": { "line": query.position.0, "character": query.position.1 },
        });
        if operation == Operation::FindReferences {
            params["context"] = json!({ "includeDeclaration": true });
        }
        let outcome = self
            .exchange(operation.method(), params, deadline, cancel)
            .await;
        if self.state.borrow().ended.is_none() {
            let _ = self
                .notify("textDocument/didClose", json!({ "textDocument": document }))
                .await;
        }
        normalize(operation, outcome?).ok_or_else(|| {
            LspError::failed(format!(
                "{id} sent a malformed {} result.",
                operation.name()
            ))
        })
    }

    /// Asks the server to shut down and exit, then stops the process if it
    /// does not.
    pub async fn shutdown(&self) {
        if !self.failed()
            && let Ok((_, reply)) = self.request("shutdown", Value::Null).await
            && timeout(SHUTDOWN_GRACE, reply).await.is_ok()
        {
            let _ = self.notify("exit", Value::Null).await;
        }
        self.stdin.lock().await.take();
        {
            let mut child = self.child.lock().await;
            if timeout(SHUTDOWN_GRACE, child.wait()).await.is_err() {
                let _ = child.kill().await;
            }
        }
        self.tasks.close();
        let _ = timeout(SHUTDOWN_GRACE, self.tasks.wait()).await;
    }

    /// [`Self::shutdown`] after the query in progress, if any.
    pub async fn retire(&self) {
        let _turn = self.turn.lock().await;
        self.shutdown().await;
    }

    async fn initialize(self: Arc<Self>) {
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "dim", "version": env!("CARGO_PKG_VERSION") },
            "rootUri": self.folder["uri"],
            "workspaceFolders": [self.folder],
            "capabilities": client_capabilities(),
            "initializationOptions": self.launch.initialization_options,
        });
        let outcome = match timeout(INITIALIZE_TIMEOUT, self.call("initialize", params)).await {
            Ok(Ok(result)) => self.initialized(result).await,
            Ok(Err(Fault::Ended(reason))) => Err(reason),
            Ok(Err(Fault::Error(message))) => Err(format!("refused to initialize: {message}")),
            Err(_) => Err(format!(
                "did not answer initialize within {} seconds",
                INITIALIZE_TIMEOUT.as_secs()
            )),
        };
        let outcome = outcome.map_err(|reason| {
            LspError::unavailable(format!("{} could not start: {reason}", self.launch.id))
        });
        self.state
            .send_modify(|state| state.initialized = Some(outcome));
    }

    async fn initialized(&self, result: Value) -> Result<Initialized, String> {
        let capabilities = result
            .get("capabilities")
            .filter(|capabilities| capabilities.is_object())
            .cloned()
            .ok_or("answered initialize without capabilities")?;
        if let Some(encoding) = capabilities.get("positionEncoding").and_then(Value::as_str)
            && encoding != "utf-16"
        {
            return Err(format!(
                "chose the {encoding} position encoding, and dim sends utf-16"
            ));
        }
        self.notify("initialized", json!({}))
            .await
            .map_err(|fault| match fault {
                Fault::Ended(reason) | Fault::Error(reason) => reason,
            })?;
        Ok(Initialized {
            capabilities,
            waits_for_quiescence: result.pointer("/serverInfo/name").and_then(Value::as_str)
                == Some("rust-analyzer"),
        })
    }

    async fn ready(
        &self,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<Initialized, LspError> {
        let id = &self.launch.id;
        let mut state = self.state.subscribe();
        let state = tokio::select! {
            state = state.wait_for(State::settled) => {
                state.expect("the server keeps its state sender").clone()
            }
            () = sleep_until(deadline) => {
                let starting = self.state.borrow().initialized.is_none();
                return Err(LspError::timeout(if starting {
                    format!("{id} is still starting; try again shortly.")
                } else {
                    format!("{id} is still loading the workspace; try again shortly.")
                }));
            }
            () = cancel.cancelled() => return Err(LspError::cancelled()),
        };
        match (state.initialized, state.ended) {
            (Some(Err(error)), _) => Err(error),
            (None, Some(reason)) => Err(LspError::unavailable(format!(
                "{id} could not start: {reason}"
            ))),
            (Some(Ok(_)), Some(reason)) => Err(LspError::failed(format!("{id} {reason}"))),
            (Some(Ok(initialized)), None) => Ok(initialized),
            (None, None) => unreachable!("a settled state is initialized or ended"),
        }
    }

    /// Sends one request and waits for its reply; on cancellation or the
    /// deadline it cancels the request and stops a server that keeps working
    /// on it, so a later query never overlaps it.
    async fn exchange(
        &self,
        method: &str,
        params: Value,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<Value, LspError> {
        let (request, mut reply) = self
            .request(method, params)
            .await
            .map_err(|fault| self.failure(fault))?;
        let stopped = tokio::select! {
            reply = &mut reply => {
                return reply
                    .unwrap_or_else(|_| Err(self.fault()))
                    .map_err(|fault| self.failure(fault));
            }
            () = sleep_until(deadline) => {
                LspError::timeout(format!("{} did not answer in time.", self.launch.id))
            }
            () = cancel.cancelled() => LspError::cancelled(),
        };
        let _ = self
            .notify("$/cancelRequest", json!({ "id": request }))
            .await;
        if timeout(CANCEL_GRACE, reply).await.is_err() {
            self.pending.lock().unwrap().remove(&request);
            self.stop("stopped answering a cancelled request");
        }
        Err(stopped)
    }

    async fn call(&self, method: &str, params: Value) -> Reply {
        let (_, reply) = self.request(method, params).await?;
        reply.await.unwrap_or_else(|_| Err(self.fault()))
    }

    async fn request(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(u64, oneshot::Receiver<Reply>), Fault> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, reply) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, sender);
        let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        if let Err(fault) = self.send(&message).await {
            self.pending.lock().unwrap().remove(&id);
            return Err(fault);
        }
        Ok((id, reply))
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), Fault> {
        let mut message = json!({ "jsonrpc": "2.0", "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        self.send(&message).await
    }

    async fn send(&self, message: &Value) -> Result<(), Fault> {
        let body = serde_json::to_vec(message).expect("JSON values serialize");
        let mut stdin = self.stdin.lock().await;
        let Some(pipe) = stdin.as_mut() else {
            return Err(self.fault());
        };
        let written = async {
            pipe.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
                .await?;
            pipe.write_all(&body).await?;
            pipe.flush().await
        }
        .await;
        drop(stdin);
        if let Err(error) = written {
            self.stop(&format!("stopped reading its input ({error})"));
            return Err(self.fault());
        }
        Ok(())
    }

    fn receive(self: Arc<Self>, message: Value) {
        match (
            message.get("method").and_then(Value::as_str),
            message.get("id"),
        ) {
            (Some(method), Some(id)) => {
                let reply = match self.answer(method, message.get("params")) {
                    Some(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    None => json!({ "jsonrpc": "2.0", "id": id, "error": {
                        "code": -32601,
                        "message": format!("dim does not handle {method}"),
                    }}),
                };
                let server = self.clone();
                self.tasks.spawn(async move {
                    let _ = server.send(&reply).await;
                });
            }
            (Some("experimental/serverStatus"), None) => {
                let params = message.get("params");
                let field = |name: &str| params.and_then(|params| params.get(name));
                let report = Report {
                    quiescent: field("quiescent").and_then(Value::as_bool) == Some(true),
                    health: field("health")
                        .and_then(Value::as_str)
                        .unwrap_or("ok")
                        .to_owned(),
                    message: field("message").and_then(Value::as_str).map(str::to_owned),
                };
                self.state.send_modify(|state| state.report = Some(report));
            }
            (None, Some(id)) => {
                let Some(sender) = id
                    .as_u64()
                    .and_then(|id| self.pending.lock().unwrap().remove(&id))
                else {
                    return;
                };
                let _ = sender.send(match message.get("error") {
                    Some(error) => Err(Fault::Error(
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown error")
                            .to_owned(),
                    )),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                });
            }
            _ => {}
        }
    }

    /// The result for a request the server sends; `None` is "method not
    /// found". Nothing the server asks changes a file.
    fn answer(&self, method: &str, params: Option<&Value>) -> Option<Value> {
        match method {
            "workspace/configuration" => Some(Value::Array(vec![
                Value::Null;
                params
                    .and_then(|params| params.get("items"))
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
            ])),
            "workspace/workspaceFolders" => Some(json!([self.folder])),
            "workspace/applyEdit" => Some(json!({
                "applied": false,
                "failureReason": "dim's lsp tool is read-only.",
            })),
            "window/workDoneProgress/create"
            | "window/showMessageRequest"
            | "client/registerCapability"
            | "client/unregisterCapability" => Some(Value::Null),
            method if method.starts_with("workspace/") && method.ends_with("/refresh") => {
                Some(Value::Null)
            }
            _ => None,
        }
    }

    /// Marks the transport ended and stops the process; the pool replaces
    /// the server at the next call.
    fn stop(&self, reason: &str) {
        self.state.send_modify(|state| {
            state.ended.get_or_insert_with(|| reason.to_owned());
        });
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.start_kill();
        }
    }

    async fn end(&self, how: String) {
        self.stdin.lock().await.take();
        let status = match self.child.try_lock() {
            Ok(mut child) => timeout(Duration::from_millis(500), child.wait())
                .await
                .ok()
                .and_then(Result::ok),
            Err(_) => None,
        };
        let stderr_done = self.stderr_done.lock().unwrap().take();
        if let Some(done) = stderr_done {
            let _ = timeout(Duration::from_millis(500), done).await;
        }
        let tail = String::from_utf8_lossy(&self.stderr.lock().unwrap())
            .trim()
            .to_owned();
        let mut reason = status.map_or(how, |status| format!("exited ({status})"));
        if !tail.is_empty() {
            reason = format!("{reason}: {tail}");
        }
        self.state.send_modify(|state| {
            state.ended.get_or_insert(reason);
        });
        let fault = self.state.borrow().ended.clone().unwrap_or_default();
        for (_, sender) in self.pending.lock().unwrap().drain() {
            let _ = sender.send(Err(Fault::Ended(fault.clone())));
        }
    }

    fn fault(&self) -> Fault {
        Fault::Ended(
            self.state
                .borrow()
                .ended
                .clone()
                .unwrap_or_else(|| "stopped".into()),
        )
    }

    fn failure(&self, fault: Fault) -> LspError {
        LspError::failed(match fault {
            Fault::Ended(reason) => format!("{} {reason}", self.launch.id),
            Fault::Error(message) => {
                format!("{} answered with an error: {message}", self.launch.id)
            }
        })
    }
}

async fn read(server: Weak<Server>, stdout: ChildStdout) {
    let mut reader = BufReader::new(stdout);
    let how = loop {
        match next_message(&mut reader).await {
            Ok(message) => match server.upgrade() {
                Some(server) => server.receive(message),
                None => return,
            },
            Err(how) => break how,
        }
    };
    if let Some(server) = server.upgrade() {
        server.end(how).await;
    }
}

async fn next_message(reader: &mut BufReader<ChildStdout>) -> Result<Value, String> {
    let mut length = None;
    loop {
        let mut line = String::new();
        let read = (&mut *reader)
            .take(MAX_HEADER_BYTES)
            .read_line(&mut line)
            .await
            .map_err(|error| format!("sent unreadable output ({error})"))?;
        if read == 0 {
            return Err("closed its output".into());
        }
        if !line.ends_with('\n') {
            return Err("sent a malformed message header".into());
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length.ok_or("sent a message without Content-Length")?;
    if length > MAX_MESSAGE_BYTES {
        return Err(format!("sent a message over {MAX_MESSAGE_BYTES} bytes"));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|_| "closed its output".to_owned())?;
    serde_json::from_slice(&body).map_err(|_| "sent a message that is not JSON".to_owned())
}

async fn drain(mut stderr: ChildStderr, tail: Arc<Mutex<Vec<u8>>>, done: oneshot::Sender<()>) {
    let mut chunk = [0; 1024];
    while let Ok(read) = stderr.read(&mut chunk).await
        && read > 0
    {
        let mut tail = tail.lock().unwrap();
        tail.extend_from_slice(&chunk[..read]);
        let excess = tail.len().saturating_sub(STDERR_TAIL_BYTES);
        tail.drain(..excess);
    }
    let _ = done.send(());
}

fn client_capabilities() -> Value {
    json!({
        "general": { "positionEncodings": ["utf-16"] },
        "workspace": { "workspaceFolders": true },
        "textDocument": {
            "synchronization": { "dynamicRegistration": false },
            "hover": { "contentFormat": ["markdown", "plaintext"] },
            "definition": { "linkSupport": true },
            "implementation": { "linkSupport": true },
            "references": {},
        },
        "experimental": { "serverStatusNotification": true },
    })
}

fn normalize(operation: Operation, payload: Value) -> Option<Answer> {
    match operation {
        Operation::Hover => hover(&payload).map(Answer::Hover),
        _ => locations(payload).map(Answer::Locations),
    }
}

/// `Location`, `Location[]` or `LocationLink[]`; a link's target is its
/// selection range.
fn locations(payload: Value) -> Option<Vec<Location>> {
    let items = match payload {
        Value::Null => Vec::new(),
        Value::Array(items) => items,
        item => vec![item],
    };
    items
        .iter()
        .map(|item| {
            let (uri, range) = match item.get("targetUri") {
                Some(uri) => (uri, item.get("targetSelectionRange")?),
                None => (item.get("uri")?, item.get("range")?),
            };
            Some(Location {
                uri: uri.as_str()?.to_owned(),
                line: range.pointer("/start/line")?.as_u64()?,
                character: range.pointer("/start/character")?.as_u64()?,
            })
        })
        .collect()
}

/// `MarkupContent`, a `MarkedString` or an array of them; a language-tagged
/// string becomes a fenced block.
fn hover(payload: &Value) -> Option<Option<String>> {
    if payload.is_null() {
        return Some(None);
    }
    let text = marked(payload.get("contents")?)?;
    let text = text.trim();
    Some((!text.is_empty()).then(|| text.to_owned()))
}

fn marked(contents: &Value) -> Option<String> {
    match contents {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => parts
            .iter()
            .map(marked)
            .collect::<Option<Vec<_>>>()
            .map(|parts| parts.join("\n\n")),
        Value::Object(object) => {
            let value = object.get("value")?.as_str()?;
            if object.contains_key("kind") {
                Some(value.to_owned())
            } else {
                let language = object.get("language")?.as_str()?;
                Some(format!("```{language}\n{value}\n```"))
            }
        }
        _ => None,
    }
}
