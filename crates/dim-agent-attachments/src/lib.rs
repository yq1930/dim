//! Attachments ([design](../../../docs/design/plugins/Attachments.md)): the
//! Session method `attachment.put`, which stores a large paste or a PDF as a
//! Session file through SessionStore's `sessionFiles` and returns its path. The
//! model reads the file with `read` like any file; images need no plugin.
use base64::{Engine, engine::general_purpose::STANDARD};
use dim_agent_capabilities::{
    host::{SessionFileError, SessionFileErrorCode, SessionFilesHost},
    methods::{self, Interruption, MethodDecl, RpcError, SessionMethods},
};
use dim_agent_runtime::{AgentBuilder, Names, SessionIdentity};
use rutis::Ctx;
use rutis::host::{BoxFuture, CtxHost, DynPlugin, Effect, Key, TypeKey};
use std::{path::PathBuf, sync::Arc};

/// The `sessionMethods` namespace Attachments provides.
const NAMESPACE: &str = "attachment";
/// The content limit of one put (A2); SessionStore's own cap is 16 MiB.
const MAX_BYTES: usize = 10 * 1024 * 1024;

/// Installs Attachments in the profile. A host calls this only where
/// SessionStore provides `sessionFiles` and the model has `read`.
pub fn install(builder: &mut AgentBuilder) {
    builder.require_feature(
        dim_agent_runtime::Feature::new("Attachments").session("Attachments", |names| {
            Ok(Box::new(AttachmentsPlugin::new(names)?))
        }),
    );
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PutParams {
    /// Read by Rpc to route the call; the provider stores for its own Session.
    #[serde(rename = "sessionId")]
    _session_id: String,
    name: Option<String>,
    text: Option<String>,
    /// A PDF's bytes, in place of `text`.
    pdf_base64: Option<String>,
}

/// SessionStore's reply passed through, with `attachmentId = fileId`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PutReply {
    attachment_id: String,
    path: PathBuf,
    bytes: u64,
}

/// `attachment.put` errors for SessionStore's codes (design A.2).
fn put_error(error: SessionFileError) -> RpcError {
    let code = match error.code {
        SessionFileErrorCode::NotFound => return RpcError::closed(),
        SessionFileErrorCode::Cancelled => return RpcError::reloading(),
        SessionFileErrorCode::Invalid => "attachment_invalid",
        SessionFileErrorCode::TooLarge => "attachment_too_large",
        SessionFileErrorCode::NodeUnavailable => "node_unavailable",
        SessionFileErrorCode::Storage => "storage_failed",
    };
    RpcError::app(-32000, code, error.message)
}

pub struct AttachmentsPlugin {
    identity: Key<SessionIdentity>,
    files: Key<SessionFilesHost>,
    methods: Key<SessionMethods>,
}
impl AttachmentsPlugin {
    pub fn new(names: &Names<'_>) -> rutis::Result<Self> {
        Ok(Self {
            identity: names.key()?,
            files: names.key()?,
            methods: names.key()?,
        })
    }
}
impl DynPlugin for AttachmentsPlugin {
    fn name(&self) -> &str {
        "Attachments"
    }
    fn injects(&self) -> Vec<TypeKey> {
        vec![
            self.identity.erase(),
            self.files.erase(),
            self.methods.erase(),
        ]
    }
    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let identity = ctx.require_at(&self.identity)?;
            let files = ctx.require_at(&self.files)?;
            let methods = ctx.require_at(&self.methods)?;
            let session_id = identity.session_id.clone();
            methods.register(
                ctx,
                NAMESPACE,
                vec![MethodDecl::new("attachment.put", true, Interruption::Drain)],
                Arc::new(move |_, value, cancel, _| {
                    let (files, session_id) = (files.clone(), session_id.clone());
                    Box::pin(async move {
                        let PutParams {
                            name,
                            text,
                            pdf_base64,
                            ..
                        } = methods::params(value)?;
                        let invalid =
                            |message| RpcError::app(-32000, "attachment_invalid", message);
                        let (default_name, bytes) = match (text, pdf_base64) {
                            (Some(text), None) if text.is_empty() => {
                                return Err(invalid("Attachment text is empty"));
                            }
                            (Some(text), None) => ("paste.txt", text.into_bytes()),
                            (None, Some(encoded)) => {
                                let bytes = STANDARD
                                    .decode(encoded)
                                    .map_err(|_| invalid("Attachment PDF is not valid base64"))?;
                                if !bytes.starts_with(b"%PDF-") {
                                    return Err(invalid("Attachment is not a PDF"));
                                }
                                ("attachment.pdf", bytes)
                            }
                            _ => {
                                return Err(RpcError::invalid(
                                    "attachment.put takes exactly one of text and pdfBase64",
                                ));
                            }
                        };
                        if bytes.len() > MAX_BYTES {
                            return Err(RpcError::app(
                                -32000,
                                "attachment_too_large",
                                "Attachment exceeds 10 MiB",
                            ));
                        }
                        let stored = files
                            .0
                            .put(
                                session_id,
                                name.unwrap_or_else(|| default_name.into()),
                                bytes,
                                cancel,
                            )
                            .await
                            .map_err(put_error)?;
                        methods::reply(PutReply {
                            attachment_id: stored.file_id,
                            path: stored.path,
                            bytes: stored.bytes,
                        })
                    })
                }),
            )?;
            Ok(Effect::Done)
        })
    }
}
