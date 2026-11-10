//! `scratch.*` management methods: the directories of the Sessions the Web
//! workspace starts without a folder
//! ([web workspace §4.11](../../../docs/design/web-workspace.md#411-sessions-without-a-folder)).
//! Each such Session gets a directory of its own under
//! `<product home>/scratch`.
use crate::methods::ManagementMethods;
use dim_agent_capabilities::methods::{Interruption, MethodDecl, RpcError, params, reply};
use dim_agent_runtime::Names;
use rutis::Ctx;
use rutis::host::{BoxFuture, CtxHost, DynPlugin, Effect, Key, TypeKey};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

const NAMESPACE: &str = "scratch";
const ROOT: &str = "scratch.root";
const CREATE: &str = "scratch.create";
const MAX_NAME: usize = 64;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateRequest {
    name: String,
}

/// `<product home>/scratch` as a Session's canonical `cwd` spells it, so a
/// client can tell these directories by prefix.
async fn root(home: Option<&Path>) -> Result<PathBuf, RpcError> {
    let home = home.ok_or_else(unavailable)?;
    let home = tokio::fs::canonicalize(home)
        .await
        .map_err(|error| failed(&error))?;
    Ok(home.join("scratch"))
}

async fn create(home: Option<&Path>, request: CreateRequest) -> Result<Value, RpcError> {
    let name = request.name;
    if name.is_empty()
        || name.len() > MAX_NAME
        || name.starts_with('.')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(RpcError::invalid(
            "name must be ASCII letters, digits, '-', '_' or '.'",
        ));
    }
    let root = root(home).await?;
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|error| failed(&error))?;
    let path = root.join(name);
    tokio::fs::create_dir(&path).await.map_err(|error| {
        if error.kind() == ErrorKind::AlreadyExists {
            RpcError::app(
                -32000,
                "scratch_exists",
                format!("{} already exists", path.display()),
            )
        } else {
            failed(&error)
        }
    })?;
    reply(json!({ "path": text(path)? }))
}

fn text(path: PathBuf) -> Result<String, RpcError> {
    path.into_os_string().into_string().map_err(|_| {
        RpcError::app(
            -32000,
            "scratch_failed",
            "The product home path is not UTF-8",
        )
    })
}

fn failed(error: &std::io::Error) -> RpcError {
    RpcError::app(-32000, "scratch_failed", error.to_string())
}

fn unavailable() -> RpcError {
    RpcError::app(
        -32000,
        "scratch_unavailable",
        "Scratch directories need the product home",
    )
}

pub(crate) struct ScratchPlugin {
    methods: Key<ManagementMethods>,
    product_home: Option<PathBuf>,
}

impl ScratchPlugin {
    pub(crate) fn new(names: &Names<'_>, product_home: Option<PathBuf>) -> rutis::Result<Self> {
        Ok(Self {
            methods: names.key()?,
            product_home,
        })
    }
}

impl DynPlugin for ScratchPlugin {
    fn name(&self) -> &str {
        "Scratch"
    }

    fn injects(&self) -> Vec<TypeKey> {
        vec![self.methods.erase()]
    }

    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let methods = ctx.require_at(&self.methods)?;
            let home: Arc<Option<PathBuf>> = Arc::new(self.product_home.clone());
            methods.register(
                ctx,
                NAMESPACE,
                vec![
                    MethodDecl::new(ROOT, false, Interruption::Report),
                    MethodDecl::new(CREATE, true, Interruption::Drain),
                ],
                Arc::new(move |method, value, _, _| {
                    let home = home.clone();
                    Box::pin(async move {
                        let home = home.as_deref();
                        match method.as_str() {
                            ROOT => reply(json!({ "root": text(root(home).await?)? })),
                            CREATE => create(home, params(value)?).await,
                            _ => unreachable!("registered scratch method"),
                        }
                    })
                }),
            )?;
            Ok(Effect::Done)
        })
    }
}
