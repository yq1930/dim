use super::{FileTools, blocking, cancelled, invalid_input, resolve_path, succeeded};
use crate::host::HostError;
use dim_agent_types::{JsonObject, ToolDefinition, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use tokio_util::sync::CancellationToken;

const MAX_FILES: usize = 8;

pub(in crate::workspace_tools) fn definition() -> ToolDefinition {
    serde_json::from_value(json!({
        "name": "present",
        "description": "Deliver existing final files to the user at the end of the turn, including files made by commands. Use this for requested outputs such as reports, images, or archives. Paths must be in the workspace or this session's output directory.",
        "inputSchema": {
            "type": "object",
            "required": ["files"],
            "additionalProperties": false,
            "properties": {
                "files": {
                    "type": "array", "minItems": 1, "maxItems": MAX_FILES,
                    "items": {
                        "type": "object", "required": ["path"], "additionalProperties": false,
                        "properties": {
                            "path": { "type": "string", "description": "Path to the finished file." },
                            "description": { "type": "string", "maxLength": 400, "description": "Optional short description for the user." }
                        }
                    }
                }
            }
        },
        "source": "builtin",
        "risk": "read_filesystem",
    })).expect("the present tool definition is valid")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    files: Vec<File>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

pub(in crate::workspace_tools) async fn run(
    tools: &FileTools,
    input: &JsonObject,
    cancel: CancellationToken,
) -> Result<ToolResult, HostError> {
    let Ok(Input { files }) = serde_json::from_value(Value::Object(input.clone())) else {
        return Ok(invalid_input(
            "present requires a list of files with paths and optional descriptions.",
        ));
    };
    if files.is_empty() || files.len() > MAX_FILES {
        return Ok(invalid_input(format!(
            "present requires 1 to {MAX_FILES} files."
        )));
    }
    let (cwd, home, output) = (
        tools.cwd.clone(),
        tools.home.clone(),
        tools.output_dir.clone(),
    );
    blocking(move || {
        let workspace = std::fs::canonicalize(&cwd).ok();
        let output = std::fs::canonicalize(output).ok();
        let mut seen = HashSet::new();
        let mut delivered = Vec::new();
        for file in files {
            if cancel.is_cancelled() {
                return Err(cancelled());
            }
            if file.path.trim().is_empty()
                || file
                    .description
                    .as_ref()
                    .is_some_and(|text| text.chars().count() > 400)
            {
                return Ok(invalid_input(
                    "Each file needs a path and a description of at most 400 characters.",
                ));
            }
            let path = match resolve_path(&cwd, &home, &file.path) {
                Ok(path) => path,
                Err(error) => return Ok(invalid_input(error)),
            };
            if !workspace
                .iter()
                .chain(output.iter())
                .any(|root| path.starts_with(root))
            {
                return Ok(invalid_input(format!(
                    "{} is outside the workspace and session output directory.",
                    file.path
                )));
            }
            if !path.is_file() {
                return Ok(invalid_input(format!(
                    "{} is not an existing regular file.",
                    file.path
                )));
            }
            let Ok(path) = path.into_os_string().into_string() else {
                return Ok(invalid_input("A delivered path must be valid UTF-8."));
            };
            if seen.insert(path.clone()) {
                delivered.push(File {
                    path,
                    description: file.description.filter(|text| !text.trim().is_empty()),
                });
            }
        }
        let message = delivered
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(succeeded(
            format!("Delivered files:\n{message}"),
            json!({ "dimDeliveredFiles": delivered }),
        ))
    })
    .await?
}
