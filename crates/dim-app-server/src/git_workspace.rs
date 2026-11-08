//! `git.*` management methods: what Git says about a Session's directory
//! (its repository, main worktree, branch, pull request and checks), and the
//! worktrees the Web workspace starts Sessions in
//! ([web workspace §4.5](../../../docs/design/web-workspace.md#45-git)).
//! Worktrees are created under `<product home>/worktrees`; only those can be
//! removed here.
use crate::methods::ManagementMethods;
use dim_agent_capabilities::methods::{Interruption, MethodDecl, RpcError, params, reply};
use dim_agent_runtime::Names;
use rutis::Ctx;
use rutis::host::{BoxFuture, CtxHost, DynPlugin, Effect, Key, TypeKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::process::Command;

const NAMESPACE: &str = "git";
const INSPECT: &str = "git.inspect";
const STATUS: &str = "git.status";
const PULL_REQUEST: &str = "git.pr";
const WORKTREE_CREATE: &str = "git.worktree.create";
const WORKTREE_REMOVE: &str = "git.worktree.remove";
const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const GH_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_PATHS: usize = 200;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InspectRequest {
    paths: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CwdRequest {
    cwd: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateRequest {
    cwd: String,
    branch: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RemoveRequest {
    path: String,
}

/// Where a directory lies in Git: the worktree containing it, the
/// repository's main worktree, and the checked-out branch.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Place {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    main: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Check {
    name: String,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

fn git(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    command
}

/// The command's stdout, or why it failed: stderr, or the spawn error.
async fn run(mut command: Command, limit: Duration) -> Result<String, Failure> {
    match tokio::time::timeout(limit, command.output()).await {
        Err(_) => Err(Failure::Failed("timed out".into())),
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Err(Failure::Missing),
        Ok(Err(error)) => Err(Failure::Failed(error.to_string())),
        Ok(Ok(output)) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(Ok(output)) => Err(Failure::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        )),
    }
}

enum Failure {
    Missing,
    Failed(String),
}

fn git_failed(failure: Failure) -> RpcError {
    match failure {
        Failure::Missing => RpcError::app(-32000, "git_missing", "Git is not installed"),
        Failure::Failed(message) => RpcError::app(-32000, "git_failed", message),
    }
}

async fn place(path: &str) -> Place {
    let dir = Path::new(path);
    let mut place = Place {
        path: path.to_owned(),
        root: None,
        main: None,
        branch: None,
    };
    if !dir.is_absolute() {
        return place;
    }
    let mut command = git(dir);
    command.args([
        "rev-parse",
        "--path-format=absolute",
        "--show-toplevel",
        "--git-common-dir",
    ]);
    let Ok(found) = run(command, GIT_TIMEOUT).await else {
        return place;
    };
    let mut lines = found.lines();
    let (Some(root), Some(common)) = (lines.next(), lines.next()) else {
        return place;
    };
    let common = Path::new(common);
    let main = match common.file_name() {
        Some(name) if name == ".git" => common.parent().map(Path::to_path_buf),
        _ => None,
    }
    .unwrap_or_else(|| PathBuf::from(root));
    let mut command = git(dir);
    command.args(["symbolic-ref", "--short", "-q", "HEAD"]);
    place.branch = run(command, GIT_TIMEOUT)
        .await
        .ok()
        .map(|branch| branch.trim().to_owned())
        .filter(|branch| !branch.is_empty());
    place.root = Some(root.to_owned());
    place.main = Some(main.to_string_lossy().into_owned());
    place
}

struct GitWorkspace {
    /// `<product home>/worktrees`; absent without a product home.
    worktrees: Option<PathBuf>,
}

impl GitWorkspace {
    async fn inspect(&self, request: InspectRequest) -> Result<Value, RpcError> {
        if request.paths.len() > MAX_PATHS {
            return Err(RpcError::invalid(format!(
                "at most {MAX_PATHS} paths at a time"
            )));
        }
        let places =
            futures_util::future::join_all(request.paths.iter().map(|path| place(path))).await;
        reply(json!({ "places": places }))
    }

    fn managed(&self, root: &Path) -> bool {
        self.worktrees
            .as_deref()
            .and_then(|worktrees| std::fs::canonicalize(worktrees).ok())
            .is_some_and(|worktrees| root.starts_with(worktrees))
    }

    async fn status(&self, request: CwdRequest) -> Result<Value, RpcError> {
        let found = place(&request.cwd).await;
        let (Some(root), Some(main)) = (found.root, found.main) else {
            return Err(not_repository());
        };
        let mut command = git(Path::new(&request.cwd));
        command.args(["status", "--porcelain=v2", "--branch"]);
        let status = run(command, GIT_TIMEOUT).await.map_err(git_failed)?;
        let (mut upstream, mut ahead, mut behind, mut changed) = (None, 0u64, 0u64, 0u64);
        for line in status.lines() {
            if let Some(name) = line.strip_prefix("# branch.upstream ") {
                upstream = Some(name.to_owned());
            } else if let Some(counts) = line.strip_prefix("# branch.ab ") {
                let mut counts = counts.split(' ');
                ahead = counts
                    .next()
                    .and_then(|count| count.trim_start_matches('+').parse().ok())
                    .unwrap_or(0);
                behind = counts
                    .next()
                    .and_then(|count| count.trim_start_matches('-').parse().ok())
                    .unwrap_or(0);
            } else if !line.starts_with('#') && !line.is_empty() {
                changed += 1;
            }
        }
        let managed = root != main && self.managed(Path::new(&root));
        reply(json!({
            "root": root,
            "main": main,
            "branch": found.branch,
            "upstream": upstream,
            "ahead": ahead,
            "behind": behind,
            "changed": changed,
            "managed": managed,
        }))
    }

    async fn pull_request(&self, request: CwdRequest) -> Result<Value, RpcError> {
        let mut command = Command::new("gh");
        command
            .current_dir(&request.cwd)
            .args([
                "pr",
                "view",
                "--json",
                "number,title,state,isDraft,url,baseRefName,headRefName,reviewDecision,statusCheckRollup",
            ])
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let unavailable = |reason: &str, message: &str| {
            reply(json!({ "available": false, "reason": reason, "message": message }))
        };
        let view = match run(command, GH_TIMEOUT).await {
            Ok(view) => view,
            Err(Failure::Missing) => {
                return unavailable("gh_missing", "GitHub CLI (gh) is not installed");
            }
            Err(Failure::Failed(message)) if message.contains("no pull requests found") => {
                return reply(json!({ "available": true, "pullRequest": null }));
            }
            Err(Failure::Failed(message)) if message.contains("gh auth login") => {
                return unavailable("gh_auth", &message);
            }
            Err(Failure::Failed(message))
                if message.contains("known GitHub host") || message.contains("no git remotes") =>
            {
                return unavailable("no_github_remote", &message);
            }
            Err(Failure::Failed(message)) => return unavailable("failed", &message),
        };
        let view: Value = serde_json::from_str(&view)
            .map_err(|error| RpcError::app(-32000, "git_failed", error.to_string()))?;
        let text = |key: &str| view.get(key).and_then(Value::as_str).unwrap_or_default();
        let checks: Vec<Check> = view
            .get("statusCheckRollup")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(check).collect())
            .unwrap_or_default();
        reply(json!({
            "available": true,
            "pullRequest": {
                "number": view.get("number").and_then(Value::as_u64).unwrap_or_default(),
                "title": text("title"),
                "state": text("state").to_ascii_lowercase(),
                "draft": view.get("isDraft").and_then(Value::as_bool).unwrap_or_default(),
                "url": text("url"),
                "base": text("baseRefName"),
                "head": text("headRefName"),
                "review": Some(text("reviewDecision").to_ascii_lowercase()).filter(|review| !review.is_empty()),
                "checks": checks,
            },
        }))
    }

    async fn create_worktree(&self, request: CreateRequest) -> Result<Value, RpcError> {
        let Some(worktrees) = &self.worktrees else {
            return Err(unavailable());
        };
        let branch = request.branch.trim();
        let mut command = git(Path::new(&request.cwd));
        command.args(["check-ref-format", "--branch", branch]);
        if branch.is_empty() || branch.starts_with('-') || run(command, GIT_TIMEOUT).await.is_err()
        {
            return Err(RpcError::invalid(format!(
                "{branch:?} is not a valid branch name"
            )));
        }
        let found = place(&request.cwd).await;
        let (Some(root), Some(main)) = (found.root, found.main) else {
            return Err(not_repository());
        };
        let repository = Path::new(&main)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repository".into());
        let digest = Sha256::digest(main.as_bytes());
        let group = format!("{repository}-{}", hex(&digest[..4]));
        let leaf: String = branch
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let path = worktrees.join(group).join(leaf);
        if path.exists() {
            return Err(RpcError::app(
                -32000,
                "worktree_exists",
                format!("{} already exists", path.display()),
            ));
        }
        std::fs::create_dir_all(path.parent().expect("worktree group"))
            .map_err(|error| git_failed(Failure::Failed(error.to_string())))?;
        let mut command = git(Path::new(&root));
        command
            .args(["worktree", "add", "-b", branch, "--"])
            .arg(&path);
        run(command, GIT_TIMEOUT).await.map_err(git_failed)?;
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        reply(json!({ "path": path, "branch": branch }))
    }

    async fn remove_worktree(&self, request: RemoveRequest) -> Result<Value, RpcError> {
        let found = place(&request.path).await;
        let (Some(root), Some(main)) = (found.root, found.main) else {
            return Err(not_repository());
        };
        let canonical =
            std::fs::canonicalize(&request.path).unwrap_or_else(|_| request.path.clone().into());
        if root == main || Path::new(&root) != canonical || !self.managed(Path::new(&root)) {
            return Err(RpcError::app(
                -32000,
                "worktree_unmanaged",
                "Only worktrees the Web workspace created can be removed",
            ));
        }
        let mut command = git(Path::new(&main));
        command.args(["worktree", "remove", "--"]).arg(&root);
        run(command, GIT_TIMEOUT).await.map_err(git_failed)?;
        if let Some(group) = Path::new(&root).parent() {
            let _ = std::fs::remove_dir(group);
        }
        reply(json!({}))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn not_repository() -> RpcError {
    RpcError::app(
        -32000,
        "git_not_repository",
        "The directory is not in a Git repository",
    )
}

fn unavailable() -> RpcError {
    RpcError::app(
        -32000,
        "worktrees_unavailable",
        "Worktrees need the product home",
    )
}

/// One entry of `gh`'s `statusCheckRollup`: a check run or a commit status.
fn check(item: &Value) -> Check {
    let text = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or_default();
    let (name, state, url) = if text("__typename") == "StatusContext" {
        let state = match text("state") {
            "SUCCESS" => "success",
            "FAILURE" | "ERROR" => "failure",
            _ => "pending",
        };
        (text("context").to_owned(), state, text("targetUrl"))
    } else {
        let state = if text("status") != "COMPLETED" {
            "pending"
        } else {
            match text("conclusion") {
                "SUCCESS" => "success",
                "SKIPPED" => "skipped",
                "NEUTRAL" | "STALE" => "neutral",
                _ => "failure",
            }
        };
        let name = match text("workflowName") {
            "" => text("name").to_owned(),
            workflow => format!("{workflow} / {}", text("name")),
        };
        (name, state, text("detailsUrl"))
    };
    Check {
        name,
        state,
        url: Some(url.to_owned()).filter(|url| !url.is_empty()),
    }
}

pub(crate) struct GitWorkspacePlugin {
    methods: Key<ManagementMethods>,
    product_home: Option<PathBuf>,
}

impl GitWorkspacePlugin {
    pub(crate) fn new(names: &Names<'_>, product_home: Option<PathBuf>) -> rutis::Result<Self> {
        Ok(Self {
            methods: names.key()?,
            product_home,
        })
    }
}

impl DynPlugin for GitWorkspacePlugin {
    fn name(&self) -> &str {
        "GitWorkspace"
    }

    fn injects(&self) -> Vec<TypeKey> {
        vec![self.methods.erase()]
    }

    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let methods = ctx.require_at(&self.methods)?;
            let workspace = Arc::new(GitWorkspace {
                worktrees: self
                    .product_home
                    .as_ref()
                    .map(|home| home.join("worktrees")),
            });
            methods.register(
                ctx,
                NAMESPACE,
                vec![
                    MethodDecl::new(INSPECT, false, Interruption::Report),
                    MethodDecl::new(STATUS, false, Interruption::Report),
                    MethodDecl::new(PULL_REQUEST, false, Interruption::Report),
                    MethodDecl::new(WORKTREE_CREATE, true, Interruption::Drain),
                    MethodDecl::new(WORKTREE_REMOVE, true, Interruption::Drain),
                ],
                Arc::new(move |method, value, _, _| {
                    let workspace = workspace.clone();
                    Box::pin(async move {
                        match method.as_str() {
                            INSPECT => workspace.inspect(params(value)?).await,
                            STATUS => workspace.status(params(value)?).await,
                            PULL_REQUEST => workspace.pull_request(params(value)?).await,
                            WORKTREE_CREATE => workspace.create_worktree(params(value)?).await,
                            WORKTREE_REMOVE => workspace.remove_worktree(params(value)?).await,
                            _ => unreachable!("registered git method"),
                        }
                    })
                }),
            )?;
            Ok(Effect::Done)
        })
    }
}
