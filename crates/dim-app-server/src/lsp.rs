//! The `lsp` tool: read-only language-server queries in the Session's
//! workspace ([design](../../../docs/design/plugins/Lsp.md)).
mod server;

use crate::settings::{SettingsScope, SettingsService, SourceDeclaration};
use dim_agent_capabilities::{
    host::{HostFuture, ToolExecutionContext},
    tool_registry::{ToolContribution, ToolHandler, ToolRegistry},
};
use dim_agent_runtime::{Names, PartRegistry, PartScope, SessionConfig};
use dim_agent_types::{ToolCall, ToolContent, ToolDefinition, ToolResult, ToolSource};
use rutis::Ctx;
use rutis::host::{BoxFuture, CtxHost, DynPlugin, Effect, Key, TypeKey};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use server::{Answer, Launch, Location, Query, Server};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use url::Url;

const CALL_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_LOCATIONS: usize = 100;
const MAX_RESULT_CHARS: usize = 16_000;
const MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;

const DESCRIPTION: &str = "Ask the workspace's language server about the symbol at a position: goToDefinition, findReferences (includes the declaration), goToImplementation or hover. line and character are 1-based; character counts UTF-16 code units. Returns path:line:character locations or hover text.";

/// The `lsp` document: language servers by id. An entry replaces the
/// built-in server with its id, and claims its extensions from every
/// built-in server.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LspDocument {
    #[serde(default)]
    servers: BTreeMap<String, ServerConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServerConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    extension_to_language: BTreeMap<String, String>,
    #[serde(default)]
    initialization_options: Option<Value>,
}

pub fn source_declaration() -> SourceDeclaration {
    SourceDeclaration {
        source: "lsp".into(),
        user_file: "lsp.json".into(),
        project_file: None,
        schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "servers": {
                    "type": "object",
                    "default": {},
                    "additionalProperties": {
                        "type": "object",
                        "required": ["command", "extensionToLanguage"],
                        "additionalProperties": false,
                        "properties": {
                            "command": { "type": "string", "minLength": 1 },
                            "args": { "type": "array", "items": { "type": "string" } },
                            "env": {
                                "type": "object",
                                "additionalProperties": { "type": "string" }
                            },
                            "extensionToLanguage": {
                                "type": "object",
                                "minProperties": 1,
                                "propertyNames": { "pattern": "^\\.[a-z0-9_+-]+$" },
                                "additionalProperties": { "type": "string", "minLength": 1 }
                            },
                            "initializationOptions": {}
                        }
                    }
                }
            }
        }),
        user_only: vec!["/servers".into()],
    }
}

/// The servers a call uses when they are on PATH, unless `lsp` replaces
/// them.
fn built_in() -> [(&'static str, ServerConfig); 4] {
    let server = |command: &str,
                  args: &[&str],
                  languages: &[(&str, &str)],
                  initialization_options: Option<Value>| ServerConfig {
        command: command.into(),
        args: args.iter().map(|arg| (*arg).into()).collect(),
        env: BTreeMap::new(),
        extension_to_language: languages
            .iter()
            .map(|(extension, language)| ((*extension).into(), (*language).into()))
            .collect(),
        initialization_options,
    };
    [
        (
            "rust-analyzer",
            server(
                "rust-analyzer",
                &[],
                &[(".rs", "rust")],
                Some(json!({
                    "cargo": { "buildScripts": { "enable": false } },
                    "procMacro": { "enable": false },
                    "files": { "watcher": "server" },
                })),
            ),
        ),
        (
            "typescript-language-server",
            server(
                "typescript-language-server",
                &["--stdio"],
                &[
                    (".ts", "typescript"),
                    (".tsx", "typescriptreact"),
                    (".mts", "typescript"),
                    (".cts", "typescript"),
                    (".js", "javascript"),
                    (".jsx", "javascriptreact"),
                    (".mjs", "javascript"),
                    (".cjs", "javascript"),
                ],
                Some(json!({ "tsserver": { "useSyntaxServer": "never" } })),
            ),
        ),
        ("gopls", server("gopls", &[], &[(".go", "go")], None)),
        (
            "pyright",
            server(
                "pyright-langserver",
                &["--stdio"],
                &[(".py", "python"), (".pyi", "python")],
                None,
            ),
        ),
    ]
}

/// The server for files with `extension`: the configured one that lists it,
/// else the built-in one that lists it and is not replaced.
fn select(
    mut document: LspDocument,
    extension: &str,
) -> Result<Option<(String, ServerConfig)>, LspError> {
    let claims: Vec<String> = document
        .servers
        .iter()
        .filter(|(_, server)| server.extension_to_language.contains_key(extension))
        .map(|(id, _)| id.clone())
        .collect();
    match claims.as_slice() {
        [] => Ok(built_in()
            .into_iter()
            .find(|(id, server)| {
                !document.servers.contains_key(*id)
                    && server.extension_to_language.contains_key(extension)
            })
            .map(|(id, server)| (id.to_owned(), server))),
        [id] => Ok(document.servers.remove_entry(id)),
        [first, second, ..] => Err(LspError::settings(format!(
            "{first} and {second} both list {extension} in lsp.json."
        ))),
    }
}

#[derive(Clone, Debug)]
struct LspError {
    code: &'static str,
    message: String,
}

impl LspError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self::new("LSP_INVALID_INPUT", message)
    }
    fn unavailable(message: impl Into<String>) -> Self {
        Self::new("LSP_UNAVAILABLE", message)
    }
    fn unsupported(message: impl Into<String>) -> Self {
        Self::new("LSP_UNSUPPORTED_OPERATION", message)
    }
    fn failed(message: impl Into<String>) -> Self {
        Self::new("LSP_SERVER_FAILED", message)
    }
    fn timeout(message: impl Into<String>) -> Self {
        Self::new("LSP_TIMEOUT", message)
    }
    fn settings(message: impl Into<String>) -> Self {
        Self::new("LSP_SETTINGS_INVALID", message)
    }
    fn cancelled() -> Self {
        Self::new("LSP_CANCELLED", "Call cancelled.")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    GoToDefinition,
    FindReferences,
    GoToImplementation,
    Hover,
}

impl Operation {
    const ALL: [Self; 4] = [
        Self::GoToDefinition,
        Self::FindReferences,
        Self::GoToImplementation,
        Self::Hover,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::GoToDefinition => "goToDefinition",
            Self::FindReferences => "findReferences",
            Self::GoToImplementation => "goToImplementation",
            Self::Hover => "hover",
        }
    }
    fn method(self) -> &'static str {
        match self {
            Self::GoToDefinition => "textDocument/definition",
            Self::FindReferences => "textDocument/references",
            Self::GoToImplementation => "textDocument/implementation",
            Self::Hover => "textDocument/hover",
        }
    }
    fn capability(self) -> &'static str {
        match self {
            Self::GoToDefinition => "definitionProvider",
            Self::FindReferences => "referencesProvider",
            Self::GoToImplementation => "implementationProvider",
            Self::Hover => "hoverProvider",
        }
    }
}

/// The model's arguments with their 1-based coordinates.
struct Request {
    operation: Operation,
    path: String,
    line: u64,
    character: u64,
}

impl Request {
    fn parse(input: &Map<String, Value>) -> Result<Self, LspError> {
        if let Some(name) = input
            .keys()
            .find(|name| !["operation", "path", "line", "character"].contains(&name.as_str()))
        {
            return Err(LspError::invalid(format!("Unknown parameter {name}.")));
        }
        let operation = input
            .get("operation")
            .and_then(Value::as_str)
            .and_then(|name| Operation::ALL.into_iter().find(|op| op.name() == name))
            .ok_or_else(|| {
                LspError::invalid(
                    "operation must be goToDefinition, findReferences, goToImplementation or hover.",
                )
            })?;
        let path = input
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .ok_or_else(|| LspError::invalid("path must be a non-empty string."))?;
        let coordinate = |name: &str| {
            input
                .get(name)
                .and_then(one_based)
                .ok_or_else(|| LspError::invalid(format!("{name} must be a positive integer.")))
        };
        Ok(Self {
            operation,
            path: path.to_owned(),
            line: coordinate("line")?,
            character: coordinate("character")?,
        })
    }
}

fn one_based(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| {
            value
                .as_f64()
                .filter(|n| n.fract() == 0.0 && *n >= 1.0 && *n <= u32::MAX as f64)
                .map(|n| n as u64)
        })
        .filter(|n| *n >= 1)
}

/// The zero-based protocol position of a 1-based line and UTF-16 character.
fn position(text: &str, line: u64, character: u64) -> Result<(u64, u64), LspError> {
    let Some(content) = usize::try_from(line - 1)
        .ok()
        .and_then(|index| text.split('\n').nth(index))
    else {
        return Err(LspError::invalid(format!(
            "line {line} is past the end of the file ({} lines).",
            text.lines().count()
        )));
    };
    let width = content
        .strip_suffix('\r')
        .unwrap_or(content)
        .encode_utf16()
        .count() as u64;
    if character > width + 1 {
        return Err(LspError::invalid(format!(
            "character {character} is past the end of line {line} ({width} UTF-16 code units)."
        )));
    }
    Ok((line - 1, character - 1))
}

pub struct LspPlugin {
    settings: Key<SettingsService>,
    parts: Key<PartRegistry>,
}

impl LspPlugin {
    pub fn new(names: &Names<'_>) -> rutis::Result<Self> {
        Ok(Self {
            settings: names.key()?,
            parts: names.key()?,
        })
    }
}

impl DynPlugin for LspPlugin {
    fn name(&self) -> &str {
        "lsp"
    }

    fn injects(&self) -> Vec<TypeKey> {
        vec![self.settings.erase(), self.parts.erase()]
    }

    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let settings = ctx.require_at(&self.settings)?;
            let parts = ctx.require_at(&self.parts)?;
            settings.register_declared(ctx, source_declaration())?;
            PartRegistry::register(&parts, ctx, PartScope::Session, "Lsp", |mount| {
                Ok(Some(Box::new(LspSessionPlugin::new(mount.names)?)))
            })?;
            Ok(Effect::Done)
        })
    }
}

struct LspSessionPlugin {
    settings: Key<SettingsService>,
    tools: Key<ToolRegistry>,
    config: Key<SessionConfig>,
}

impl LspSessionPlugin {
    fn new(names: &Names<'_>) -> rutis::Result<Self> {
        Ok(Self {
            settings: names.key()?,
            tools: names.key()?,
            config: names.key()?,
        })
    }
}

impl DynPlugin for LspSessionPlugin {
    fn name(&self) -> &str {
        "Lsp"
    }

    fn injects(&self) -> Vec<TypeKey> {
        vec![
            self.settings.erase(),
            self.tools.erase(),
            self.config.erase(),
        ]
    }

    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let settings = ctx.require_at(&self.settings)?;
            let tools = ctx.require_at(&self.tools)?;
            let config = ctx.require_at(&self.config)?;
            let state = Arc::new(SessionState {
                settings,
                cwd: config.cwd.clone(),
                servers: Mutex::default(),
                closing: CancellationToken::new(),
                inflight: TaskTracker::new(),
            });
            let cleanup = state.clone();
            ctx.effect(move || async move {
                cleanup.close().await;
                Ok(())
            })?;
            tools.register(
                ctx,
                ToolContribution {
                    definition: ToolDefinition {
                        name: "lsp".into(),
                        description: DESCRIPTION.into(),
                        input_schema: serde_json::from_value(input_schema())
                            .expect("valid tool schema"),
                        source: ToolSource::Extension,
                        risk: Some("read_filesystem".into()),
                        file_changes: None,
                        metadata: None,
                    },
                    handler: Arc::new(LspToolHandler(state)),
                },
            )?;
            Ok(Effect::Done)
        })
    }
}

struct LspToolHandler(Arc<SessionState>);

impl ToolHandler for LspToolHandler {
    fn execute(
        &self,
        call: ToolCall,
        _context: ToolExecutionContext,
        cancel: CancellationToken,
    ) -> HostFuture<'_, ToolResult> {
        let state = self.0.clone();
        Box::pin(async move { Ok(state.execute(call, cancel).await) })
    }
}

/// A Session's servers, one per server id, started at their first call and
/// stopped when this part unloads.
struct SessionState {
    settings: Arc<SettingsService>,
    cwd: String,
    servers: Mutex<HashMap<String, Arc<Server>>>,
    closing: CancellationToken,
    inflight: TaskTracker,
}

impl SessionState {
    async fn execute(self: &Arc<Self>, call: ToolCall, cancel: CancellationToken) -> ToolResult {
        if self.closing.is_cancelled() || cancel.is_cancelled() {
            return error_result(&LspError::cancelled());
        }
        let owner = self.clone();
        let handle = self
            .inflight
            .spawn(async move { owner.run_call(call, cancel).await });
        handle
            .await
            .unwrap_or_else(|_| error_result(&LspError::cancelled()))
    }

    async fn run_call(&self, call: ToolCall, cancel: CancellationToken) -> ToolResult {
        let stop = self.closing.child_token();
        let answer = self.answer(&call.input, &stop);
        tokio::pin!(answer);
        let outcome = loop {
            tokio::select! {
                outcome = &mut answer => break outcome,
                () = cancel.cancelled(), if !stop.is_cancelled() => stop.cancel(),
            }
        };
        match outcome {
            Ok(text) => ToolResult {
                content: ToolContent::Text(text),
                structured_content: None,
                is_error: None,
                blob_refs: None,
                context_messages: None,
                background_task: None,
                effects: None,
            },
            Err(error) => error_result(&error),
        }
    }

    async fn answer(
        &self,
        input: &Map<String, Value>,
        stop: &CancellationToken,
    ) -> Result<String, LspError> {
        let deadline = Instant::now() + CALL_TIMEOUT;
        let request = Request::parse(input)?;
        let snapshot = self
            .settings
            .read_declared("lsp", SettingsScope::User, stop)
            .await
            .map_err(|_| {
                if stop.is_cancelled() {
                    LspError::cancelled()
                } else {
                    LspError::settings("The lsp settings could not be read.")
                }
            })?;
        let document: LspDocument = serde_json::from_value(snapshot.document)
            .map_err(|_| LspError::settings("The lsp settings are invalid."))?;
        let root = tokio::fs::canonicalize(&self.cwd).await.map_err(|_| {
            LspError::invalid(format!(
                "The Session workspace {} is not accessible.",
                self.cwd
            ))
        })?;
        let file = tokio::fs::canonicalize(root.join(&request.path))
            .await
            .map_err(|_| LspError::invalid(format!("{} does not exist.", request.path)))?;
        if !file.starts_with(&root) {
            return Err(LspError::invalid(format!(
                "{} is outside the Session workspace {}.",
                request.path,
                root.display()
            )));
        }
        let extension = file
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy().to_lowercase()))
            .unwrap_or_default();
        let Some((id, config)) = select(document, &extension)? else {
            return Err(LspError::unavailable(if extension.is_empty() {
                format!(
                    "No language server is configured or detected for {}.",
                    request.path
                )
            } else {
                format!("No language server is configured or detected for {extension} files.")
            }));
        };
        let text = read_text(&file, &request.path).await?;
        let position = position(&text, request.line, request.character)?;
        let server = self.server(Launch {
            id,
            command: config.command,
            args: config.args,
            env: config.env,
            initialization_options: config.initialization_options,
            root: root.clone(),
        })?;
        let uri = Url::from_file_path(&file).expect("a canonical path is absolute");
        let answer = server
            .query(
                &Query {
                    operation: request.operation,
                    uri: uri.as_str(),
                    language: &config.extension_to_language[&extension],
                    text: &text,
                    position,
                },
                deadline,
                stop,
            )
            .await?;
        Ok(render(answer, &root, server.health()))
    }

    fn server(&self, launch: Launch) -> Result<Arc<Server>, LspError> {
        let mut servers = self.servers.lock().unwrap();
        if let Some(server) = servers.get(&launch.id)
            && server.launch == launch
            && !server.failed()
        {
            return Ok(server.clone());
        }
        if let Some(stale) = servers.remove(&launch.id) {
            self.inflight.spawn(async move { stale.retire().await });
        }
        let server = Server::start(launch)?;
        servers.insert(server.launch.id.clone(), server.clone());
        Ok(server)
    }

    async fn close(&self) {
        self.closing.cancel();
        self.inflight.close();
        self.inflight.wait().await;
        let servers: Vec<_> = self
            .servers
            .lock()
            .unwrap()
            .drain()
            .map(|(_, server)| server)
            .collect();
        futures_util::future::join_all(servers.iter().map(|server| server.shutdown())).await;
    }
}

async fn read_text(file: &Path, shown: &str) -> Result<String, LspError> {
    let unreadable = || LspError::invalid(format!("{shown} cannot be read."));
    let metadata = tokio::fs::metadata(file).await.map_err(|_| unreadable())?;
    if !metadata.is_file() {
        return Err(LspError::invalid(format!("{shown} is not a file.")));
    }
    if metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(LspError::invalid(format!(
            "{shown} is larger than {} MiB.",
            MAX_DOCUMENT_BYTES / 1024 / 1024
        )));
    }
    let bytes = tokio::fs::read(file).await.map_err(|_| unreadable())?;
    String::from_utf8(bytes).map_err(|_| LspError::invalid(format!("{shown} is not UTF-8 text.")))
}

fn render(answer: Answer, root: &Path, health: Option<String>) -> String {
    let (text, empty) = match answer {
        Answer::Hover(Some(text)) => (text, false),
        Answer::Hover(None) => ("No hover information.".to_owned(), true),
        Answer::Locations(locations) if locations.is_empty() => ("No results.".to_owned(), true),
        Answer::Locations(locations) => (listing(&locations, root), false),
    };
    let text = match health {
        Some(health) if empty => format!("{text}\n{health}"),
        _ => text,
    };
    if text.chars().count() <= MAX_RESULT_CHARS {
        return text;
    }
    let notice = format!("\n… truncated (limit {MAX_RESULT_CHARS} characters).");
    let mut kept: String = text
        .chars()
        .take(MAX_RESULT_CHARS - notice.chars().count())
        .collect();
    kept.push_str(&notice);
    kept
}

/// `path:line:character` per location, 1-based, grouped by file in the
/// order the server gave them.
fn listing(locations: &[Location], root: &Path) -> String {
    let mut files: Vec<(String, Vec<String>)> = Vec::new();
    for location in locations.iter().take(MAX_LOCATIONS) {
        let path = shown(&location.uri, root);
        let entry = format!("{path}:{}:{}", location.line + 1, location.character + 1);
        match files.iter_mut().find(|(file, _)| *file == path) {
            Some((_, entries)) => entries.push(entry),
            None => files.push((path, vec![entry])),
        }
    }
    let mut lines: Vec<String> = files.into_iter().flat_map(|(_, entries)| entries).collect();
    let omitted = locations.len().saturating_sub(MAX_LOCATIONS);
    if omitted > 0 {
        lines.push(format!(
            "… {omitted} more locations omitted (limit {MAX_LOCATIONS})."
        ));
    }
    lines.join("\n")
}

/// A `file:` URI as a workspace-relative or absolute path; any other URI
/// as it is.
fn shown(uri: &str, root: &Path) -> String {
    let Some(path) = Url::parse(uri).ok().and_then(|url| url.to_file_path().ok()) else {
        return uri.to_owned();
    };
    match path.strip_prefix(root) {
        Ok(relative) => relative.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

fn error_result(error: &LspError) -> ToolResult {
    let mut structured = Map::new();
    structured.insert("code".into(), Value::String(error.code.into()));
    ToolResult {
        content: ToolContent::Text(format!("lsp failed: {}: {}", error.code, error.message)),
        structured_content: Some(structured),
        is_error: Some(true),
        blob_refs: None,
        context_messages: None,
        background_task: None,
        effects: None,
    }
}

fn input_schema() -> Value {
    json!({
        "type": "object",
        "required": ["operation", "path", "line", "character"],
        "additionalProperties": false,
        "properties": {
            "operation": {
                "type": "string",
                "enum": ["goToDefinition", "findReferences", "goToImplementation", "hover"]
            },
            "path": { "type": "string", "minLength": 1 },
            "line": { "type": "integer", "minimum": 1 },
            "character": { "type": "integer", "minimum": 1 }
        }
    })
}
