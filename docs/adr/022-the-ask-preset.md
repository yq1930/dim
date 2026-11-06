# 022: The ask preset

Decision status: Accepted.

## Context

[ADR 018](018-permission-is-the-preset.md) D1 offers three presets and nothing else. Users asked
for the working modes coding agents commonly offer: ask before acting, accept edits, run without
asking, and plan first. Three of them already exist. `workspace-write` accepts workspace edits and
asks for commands, `full-access` runs every call, and plan mode runs over `read-only` until the
user approves a plan ([Planning](../design/plugins/Planning.md)).

None of them asks before a workspace edit. `read-only` refuses the write and `workspace-write`
runs it, so a user who wants to review each change before it lands has no preset for that, only
the approval of calls outside the workspace.

## Decision

### D1. A fourth preset, `ask`

`ask` keeps the `workspace-write` boundary: the same read facts pass, system roots and
Session-owned artifacts are denied for writes, and a write outside the workspace asks. Within that
boundary it asks for every call that is not a read or a Session tool, workspace writes included.
Tool allows and command allow rules turn its questions into allows, as they do under
`workspace-write`.

The preset still decides both what may run and what is asked, which is ADR 018's reason for having
one control.

### D2. Rank

The presets rank `read-only < ask < workspace-write < full-access`. Caps, containment, and the
Scheduler and Workflow bounds compare in that order: `ask` caps `workspace-write` and is capped by
`read-only`.

### D3. Name and surfaces

The protocol name is `ask`. Every surface that lists the presets lists four: the CLI `--permission`
flag and `/permissions`, ACP modes, hook payloads' `permission_mode`, the Scheduler and Workflow
schemas and the plugin types. Clients present it as the manual mode; the Web composer offers the
presets and plan mode as one mode menu.

## Consequences

- ADR 018 D1 lists four presets. Its other decisions are unchanged.
- A record or request naming `ask` cannot be read by a build from before this decision, whose
  preset parsing is strict. In the current alpha that is accepted rather than migrated.
- Plan mode is unchanged: it switches to `read-only` and restores the preset it replaced, `ask`
  included.
