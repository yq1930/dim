# TimelineTree

The TimelineTree plugin registers TimelineTree in every Session and TreeTimeline in every branch.

## TimelineTree

Owner: [timeline_tree.rs](../../../../crates/dim-agent-capabilities/src/timeline_tree.rs).
Owns Session branches, reservations and asynchronous creation/restore/close,
and a line's end: the identity block in every child's opening input, the parent
block a nested line is told at open, the `info.source` a line was opened with,
`branch/end` to `branch/ended` on the parent, and the restore rule that leaves
an ended line closed
([timeline branches §5–§10](../../../design/timeline-branches.md#10-ending-a-line)).
Provides the TreeCommands/TimelineTreeControl ports. The `branch` tool is
[BranchTool](BranchTool.md)'s. Its model-facing texts — every child's identity
block and the branch markers — with their classes are listed on
[timeline branches §12.4](../../../design/timeline-branches.md#124-texts-and-their-classes).

### Dependencies

| Service | Purpose |
| --- | --- |
| `S:TimelineFactory` | Build branch capability scopes. |
| `S:SessionIdentity` | Identify the persisted Session. |
| `S:dyn TimelineStore` | Persist/load branch directories and timeline facts. |
| `S:GuideEntries` | Register the `<dim-branch-line>` and `<dim-branch-parent>` entries for the Session, until this part unloads. |

### Events and lifetime

Produces `branch/create`, `branch/create-failed`, `branch/start`, `branch/resume`,
`branch/end` and `branch/ended`, and initial child input, through tree-owned
operations. TreeTimeline consumes creation on the parent and an end on the
ending line. `TreeCommands::end`
checks only the target: it exists, it is open, and it is not `main`; a line
that ended writes nothing and answers `branch_ended`. `info` answers for an
ended line too, and `page` reads an open or ended line from the store at its
head, rebuilding stored request bodies. `batch_end`
checks a cursor against that line's atomic batch boundaries.
`query` selects one line's own facts: it forwards to the
Timeline while open and uses the ended head after closing, with the event
query's anchors, selection, ordering and paging. It rejects chain scope.

A child's opening input
is the identity block, and when the prefix it inherits holds a live parent
block, one more parent block after it naming the line that opened it at the
cut; the keyed state is read over the chain the child
inherits, never from the rendered history, while the child opens, as a resumed
line's facts are, and a failed read fails the create like any other opening
failure (branch/create-failed). Each line's node publishes the
sequence of its latest `turn/end`, which the line's BranchLine waits on for the
parent block (design
[§7.1.1](../../../design/timeline-branches.md#711-the-parent-block-branchparent)). `TreeCommands` also
answers whether a line's parent has new activity past a sequence, through the
one event query ([ADR 019](../../../adr/019-event-query.md)). Candidate branches remain private until opening
receipts/dispatch fences and initial input acceptance succeed. Creation retains
its fixed ancestry cut even if a waiter disappears. One opening budget requests
cancellation; rollback and accepted host work must still settle. Cleanup closes
the tree, cancels reservations, joins builds and closes branch contexts. Restore
settles interrupted creates without blindly replaying external work; it reads a
branch's failed creates while the branch opens, from the history its
capabilities rebuild from, and settles a child's creation from its first events
([restore-shared-read §17](../../../design/appserver/restore-shared-read.md#17-timelinetrees-restore-facts)). Tool-call
ancestry cuts cannot split required result relationships. Client detach is not
Session close, and closing is not deletion of persisted history.

### Tests

[branch_flow.rs](../../../../crates/dim-agent-capabilities/tests/branch_flow.rs):
`delayed_create_uses_its_anchor_and_survives_an_abandoned_waiter`,
`candidates_remain_private_until_the_initial_input_is_accepted`,
`session_close_cancels_an_active_build_and_waits_for_rollback`,
`ending_a_line_ends_its_children_first_and_records_one_result_each`,
`a_restart_completes_an_end_without_its_result`,
`a_host_line_opens_with_its_identity_block_and_the_creator_prompt`,
`a_line_reports_the_source_its_create_recorded_open_or_restored`.

<!-- generated:plugin TimelineTree — 由 `cargo run -p xtask -- graph` 从真实装配生成，勿手改 -->
| 项 | 值 |
| --- | --- |
| 作用域 | Session |
| 注入 | `S:GuideEntries`、`S:SessionIdentity`、`S:TimelineFactory`、`S:dyn TimelineStore` |
| 提供 | `S:TimelineTree`、`S:TreeCommands`、`S:dyn HostDispatch#treeCommands`、`S:dyn TimelineTreeControl` |
| 被注入于 | `BranchLine`、`BranchTool`、`InputMethods`、`PromptSuggestion`、`Recall`、`ShellInput`、`TreeTimeline`、`Wait`、`WorkspaceTools` |
<!-- /generated -->

## TreeTimeline

Owner: [timeline_tree.rs](../../../../crates/dim-agent-capabilities/src/timeline_tree.rs).
Routes branch-local creation facts to the Session tree using the actual parent
identity, and releases pending child reservations when the parent unloads.

### Dependencies

| Service | Purpose |
| --- | --- |
| `S:TreeCommands` | Reach the owning Session tree through its command port. |
| `B:TimelineIdentity` | Identify the parent branch. |
| `B:Dispatcher` | Consume creation events. |
| `B:Timeline` | Parent history, creation and end settlement location. |

### Events and lifetime

Consumes `branch/create` and `branch/end` (with the next turn end of a line
that ended through a tool call), and reports every `turn/end` to the tree,
which publishes it for the line's children; tree-owned work produces the
opening, failure and end facts (see TimelineTree). The handler registers creation, not a long-running build future.
Cleanup cancels pending reservations for this parent and settles their waiters.
No independent worker or persistent tree is owned here. Reload does not by
itself replay arbitrary old create events; recovery belongs to TimelineTree.
An accepted creation need not survive parent or Session close.

### Tests

[branch_flow.rs](../../../../crates/dim-agent-capabilities/tests/branch_flow.rs):
`parent_close_waits_for_dispatch_before_releasing_branch_reservations`,
`close_releases_an_unconsumed_create_without_waiting_for_dispatch`.

<!-- generated:plugin TreeTimeline — 由 `cargo run -p xtask -- graph` 从真实装配生成，勿手改 -->
| 项 | 值 |
| --- | --- |
| 作用域 | 分支 |
| 注入 | `B:Dispatcher`、`B:Timeline`、`B:TimelineIdentity`、`S:TreeCommands` |
| 提供 | — |
| 被注入于 | — |
<!-- /generated -->
