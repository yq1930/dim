use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionPreset {
    #[default]
    ReadOnly,
    /// The workspace-write boundary with every write asked for, workspace
    /// writes included ([ADR 022](../../../docs/adr/022-the-ask-preset.md)).
    Ask,
    WorkspaceWrite,
    FullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileAccess {
    ReadOnly,
    WorkspaceReadWrite,
    Unrestricted,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPolicy {
    #[serde(default)]
    pub preset: PermissionPreset,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
}

impl PermissionPolicy {
    /// The launch cap (sessions service §6.1): the lower preset, the union of
    /// the disallowed tools, and only the allowed tools both sides allow
    /// unconditionally. It is never wider than either side.
    pub fn cap(&self, bound: &PermissionPolicy) -> PermissionPolicy {
        fn allows(policy: &PermissionPolicy, tool: &String) -> bool {
            policy.preset == PermissionPreset::FullAccess || policy.allowed_tools.contains(tool)
        }
        let mut disallowed_tools = Vec::new();
        for tool in self.disallowed_tools.iter().chain(&bound.disallowed_tools) {
            if !disallowed_tools.contains(tool) {
                disallowed_tools.push(tool.clone());
            }
        }
        let mut allowed_tools = Vec::new();
        for tool in self.allowed_tools.iter().chain(&bound.allowed_tools) {
            if allows(self, tool)
                && allows(bound, tool)
                && !disallowed_tools.contains(tool)
                && !allowed_tools.contains(tool)
            {
                allowed_tools.push(tool.clone());
            }
        }
        PermissionPolicy {
            preset: std::cmp::min_by_key(self.preset, bound.preset, |preset| preset.rank()),
            allowed_tools,
            disallowed_tools,
        }
    }

    /// Whether capping this policy by `bound` changes nothing; tool lists are
    /// compared as sets.
    pub fn within(&self, bound: &PermissionPolicy) -> bool {
        let capped = self.cap(bound);
        let same = |left: &[String], right: &[String]| {
            left.iter().all(|name| right.contains(name))
                && right.iter().all(|name| left.contains(name))
        };
        self.preset == capped.preset
            && same(&self.allowed_tools, &capped.allowed_tools)
            && same(&self.disallowed_tools, &capped.disallowed_tools)
    }
}

impl PermissionPreset {
    /// The order `read-only < ask < workspace-write < full-access`.
    pub fn rank(self) -> u8 {
        match self {
            PermissionPreset::ReadOnly => 0,
            PermissionPreset::Ask => 1,
            PermissionPreset::WorkspaceWrite => 2,
            PermissionPreset::FullAccess => 3,
        }
    }

    pub fn file_access(self) -> FileAccess {
        match self {
            PermissionPreset::ReadOnly => FileAccess::ReadOnly,
            PermissionPreset::Ask | PermissionPreset::WorkspaceWrite => {
                FileAccess::WorkspaceReadWrite
            }
            PermissionPreset::FullAccess => FileAccess::Unrestricted,
        }
    }

    /// The preset's protocol name.
    pub fn name(self) -> &'static str {
        match self {
            PermissionPreset::ReadOnly => "read-only",
            PermissionPreset::Ask => "ask",
            PermissionPreset::WorkspaceWrite => "workspace-write",
            PermissionPreset::FullAccess => "full-access",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionChangedData {
    pub policy: PermissionPolicy,
}

/// Characters that end a simple command: shell separators, redirection and
/// substitution. A command rule never contains one.
pub const COMMAND_SEPARATORS: &[char] = &[';', '&', '|', '`', '(', ')', '<', '>', '$', '\n', '\r'];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandMatch {
    Exact,
    Prefix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandRuleDecision {
    Allow,
    Deny,
}

/// A user's rule for commands of process-starting tool calls; `command` is
/// compared word by word.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandRule {
    pub command: String,
    #[serde(rename = "match")]
    pub matching: CommandMatch,
    pub decision: CommandRuleDecision,
}
