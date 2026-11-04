//! The MCP protocol both clients share: catalog and Session state reports,
//! sign-in targets and replies, error hints, and the loopback listener that
//! catches a browser callback. Commands that print or prompt are in
//! `mcp_command` (line client) and `session_host` (TUI).

use crate::{Result, client::Client, sanitize::safe_line};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub user_revision: String,
    pub project_revision: Option<String>,
    pub servers: Vec<Server>,
    /// Entries that did not decode. A daemon older than this list reports
    /// none; its catalog omitted their whole source instead.
    #[serde(default)]
    pub invalid: Vec<Invalid>,
    pub user_application: Application,
    /// The plugin source's application row; a daemon older than that row
    /// reports none.
    #[serde(default)]
    pub plugin_application: Option<Application>,
    pub project_application: Option<Application>,
}
/// One entry that did not decode, with an error that names the entry and the
/// field and never repeats a value.
#[derive(Clone, Deserialize)]
pub struct Invalid {
    pub id: String,
    pub source: String,
    pub error: String,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: String,
    /// `user`, `project` or `plugin`.
    pub source: String,
    pub enabled: bool,
    pub transport: String,
    pub auth: Option<Auth>,
    /// Names only, sorted. A daemon older than this field reports none.
    #[serde(default)]
    pub referenced_variables: Vec<String>,
    /// Names only, sorted. A daemon older than this field reports none.
    #[serde(default)]
    pub unset_variables: Vec<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Auth {
    /// `required`, `authorized` or `expired`.
    pub status: String,
    pub missing_scopes: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub error: Option<String>,
    pub cleanup_error: Option<String>,
}

/// What `mcp list` prints: the rows on stdout, then the notes on stderr.
pub async fn catalog_report(client: &mut Client, cwd: &str) -> Result<(Vec<String>, Vec<String>)> {
    let catalog: Catalog = client.typed("mcp.catalog", json!({"cwd": cwd})).await?;
    let mut rows = Vec::new();
    for server in catalog.servers {
        rows.push(format!(
            "{:?}\t{}\t{}\t{}\t{}",
            server.id,
            safe_line(&server.source),
            safe_line(&server.transport),
            if server.enabled {
                "enabled"
            } else {
                "disabled"
            },
            safe_line(
                match server.auth.as_ref().map(|auth| auth.status.as_str()) {
                    Some("required") => "sign-in required",
                    Some("authorized") => "signed in",
                    Some("expired") => "sign-in expired",
                    Some(other) => other,
                    None => "—",
                }
            )
        ));
        if let Some(scopes) = server.auth.and_then(|auth| auth.missing_scopes) {
            rows.push(format!(
                "  Missing scopes: {}",
                safe_line(&scopes.join(", "))
            ));
        }
        if !server.referenced_variables.is_empty() {
            rows.push(format!(
                "  Variables: {}",
                safe_line(&server.referenced_variables.join(", "))
            ));
        }
        if !server.unset_variables.is_empty() {
            rows.push(format!(
                "  Unset variables: {}",
                safe_line(&server.unset_variables.join(", "))
            ));
        }
    }
    for entry in catalog.invalid {
        rows.push(format!(
            "{:?}\t{}\tinvalid\t{}",
            entry.id,
            safe_line(&entry.source),
            safe_line(&entry.error)
        ));
    }
    // The columns describe rows: with no row, the report says so instead.
    let mut notes = if rows.is_empty() {
        vec!["No MCP server is configured.".to_owned()]
    } else {
        vec![
            "Columns: ID, source, transport, enablement, authorization.\nAn enabled entry runs its command or endpoint. Project entries start disabled until you run mcp enable ID.\nSign in with mcp login ID; sign out with mcp logout ID.\nA row with `invalid` where its transport belongs did not decode; fix or remove it.\nVariables and unset variables are names only, never values.".to_owned(),
        ]
    };
    for (source, application) in [
        ("user", Some(catalog.user_application)),
        ("plugin", catalog.plugin_application),
        ("project", catalog.project_application),
    ] {
        if let Some(application) = application {
            for error in [application.error, application.cleanup_error]
                .into_iter()
                .flatten()
            {
                notes.push(format!("{source} MCP configuration: {}", safe_line(&error)));
            }
        }
    }
    Ok((rows, notes))
}

/// The sources an entry can be in, as `mcp list` prints them. `--source`
/// takes one of these, on the command line and in a slash command alike.
pub const SOURCES: [&str; 3] = ["user", "project", "plugin"];

/// `value` when it names one of [`SOURCES`].
pub fn source(value: &str) -> Option<&str> {
    SOURCES.into_iter().find(|source| *source == value)
}

/// Takes the `--source SOURCE` a slash command names out of its `words`,
/// wherever it stands after the verb, and returns it beside the words that
/// name the entry. The value is checked by [`source`].
pub fn take_source<'a>(
    words: &[&'a str],
) -> std::result::Result<(Option<&'a str>, Vec<&'a str>), String> {
    let mut named = None;
    let mut rest = Vec::new();
    let mut words = words.iter().copied();
    while let Some(word) = words.next() {
        if word == "--source" {
            let value = words
                .next()
                .ok_or("--source requires a value: user, project or plugin")?;
            if named.replace(value).is_some() {
                return Err("Supply --source only once".into());
            }
        } else {
            rest.push(word);
        }
    }
    match named {
        Some(value) => match source(value) {
            Some(source) => Ok((Some(source), rest)),
            None => Err(format!("--source is user, project or plugin, not {value}")),
        },
        None => Ok((None, rest)),
    }
}

/// One entry a catalog lists, valid or broken, and the source holding it.
pub enum Resolved {
    Server(Server),
    /// An entry that did not decode, with the error that keeps it out of the
    /// effective catalog.
    Broken(Invalid),
}

impl Resolved {
    pub fn source(&self) -> &str {
        match self {
            Self::Server(server) => &server.source,
            Self::Broken(entry) => &entry.source,
        }
    }

    /// The effective entry, or the error the broken one carries instead.
    pub fn server(self) -> std::result::Result<Server, String> {
        match self {
            Self::Server(server) => Ok(server),
            Self::Broken(entry) => Err(entry.error),
        }
    }
}

/// The one entry `(id, source)` names in `catalog`, valid or broken.
///
/// With a `source`, the entry that source lists, or `Ok(None)` when it lists
/// none. Without one, the entry when exactly one source lists the id, and
/// the sources when more do: the client never guesses which row the user
/// meant.
pub fn resolve(
    catalog: &Catalog,
    id: &str,
    source: Option<&str>,
) -> std::result::Result<Option<Resolved>, Vec<String>> {
    let named = |listed: &str| source.is_none_or(|source| listed == source);
    let listed: Vec<Resolved> = catalog
        .servers
        .iter()
        .filter(|server| server.id == id && named(&server.source))
        .cloned()
        .map(Resolved::Server)
        .chain(
            catalog
                .invalid
                .iter()
                .filter(|entry| entry.id == id && named(&entry.source))
                .cloned()
                .map(Resolved::Broken),
        )
        .collect();
    // The sources in catalog order, each once: a source lists one entry per
    // id, and the two lists are walked in the order the catalog reports.
    let mut sources: Vec<String> = Vec::new();
    for entry in &listed {
        let source = entry.source();
        if !sources.iter().any(|seen| seen.as_str() == source) {
            sources.push(source.to_owned());
        }
    }
    match sources.len() {
        0 => Ok(None),
        1 => Ok(listed.into_iter().next()),
        _ => Err(sources),
    }
}

pub fn error(error: crate::Error) -> crate::Error {
    let code = error
        .downcast_ref::<crate::client::RpcError>()
        .and_then(|error| error.data.as_ref())
        .and_then(|data| data.get("appError"))
        .and_then(Value::as_str);
    let action = match code {
        Some("mcp_auth_unsupported") => {
            "OAuth is unavailable for this entry; check its transport, static credentials and store format"
        }
        Some("mcp_auth_discovery_failed") => {
            "Check the server metadata or set oauth.authServerMetadataUrl"
        }
        Some("mcp_auth_client_required") => {
            "Set oauth.clientId; after an issuer change, sign out first"
        }
        Some("mcp_auth_attempt_closed") => {
            "Sign-in expired or was replaced; run mcp login ID again"
        }
        Some("mcp_auth_callback_invalid") => "Paste the full callback URL for this sign-in attempt",
        Some("mcp_auth_failed") => {
            "Authorization was refused; fix the cause and run mcp login ID again"
        }
        Some("mcp_auth_unavailable") => {
            "Authorization service or credential storage is unavailable; retry"
        }
        Some("mcp_unavailable") => {
            "Enable the MCP entry and retry; inspect mcp list for configuration errors"
        }
        _ => return error,
    };
    action.into()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub attempt_id: String,
    pub authorization_url: String,
    pub redirect_uri: String,
    pub resource: String,
    pub issuer: String,
    /// Unix milliseconds.
    pub expires_at: u64,
}

/// Each server's state in the Session, with the reason under it: a failure's
/// message, a refused tool-source version, or the catalog error for the
/// whole Session.
pub async fn session_report(client: &mut Client, session: &str) -> Result<Vec<String>> {
    let state: State = client
        .typed("session.mcp", json!({"sessionId":session}))
        .await?;
    let mut lines = Vec::new();
    for server in state.servers {
        lines.push(format!("{:?}\t{}", server.id, safe_line(&server.state)));
        if server.state == "auth_required" {
            lines.push(format!(
                "  Sign in with mcp login {:?} or /mcp login {:?}.",
                server.id, server.id
            ));
        } else if let Some(message) = &server.message {
            // `auth_required` says the same thing through its hint above.
            lines.push(format!("  {}", safe_line(message)));
        }
        if let Some(rejection) = &server.rejection {
            lines.push(format!(
                "  Version {} rejected: {}",
                safe_line(&rejection.version),
                safe_line(&rejection.reason)
            ));
        }
    }
    if let Some(error) = &state.catalog_error {
        lines.push(format!("MCP catalog: {}", safe_line(error)));
    }
    Ok(lines)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub session_id: String,
    pub servers: Vec<Status>,
    #[serde(default)]
    pub catalog_error: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub id: String,
    pub state: String,
    /// Why the server failed, from `session.mcp`.
    #[serde(default)]
    pub message: Option<String>,
    /// The last tool-source version the Session refused.
    #[serde(default)]
    pub rejection: Option<Rejection>,
}
#[derive(Deserialize)]
pub struct Rejection {
    pub version: String,
    pub reason: String,
}

pub use dim_client::callback::Capture;
