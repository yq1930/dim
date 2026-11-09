/**
 * A coordinator Session talks with the user and hands the work to team members
 * (Collaboration's `agent` tool). It mounts no tool that runs commands or writes
 * files, so delegating is the only way it gets work done; a member is its own
 * Session with its Agent's tools and the coordinator's permission. Its lines
 * would share its mount list, so `branch` is left out too, and `wait` so a turn
 * never blocks on members while messages queue.
 */
export const COORDINATOR_TOOLS: readonly string[] = [
  "read", "glob", "grep", "lsp",
  "agent", "member_approval_allow", "member_approval_reject",
  "ask_user", "todo_write", "exit_plan_mode", "goal_update", "recall",
  "skill", "schedule", "settings", "plugin",
  "web_search", "web_fetch",
]

export const COORDINATOR_PROMPT = `You coordinate this Session as the leader of a team. You talk with the user and hand the work to team members with the agent tool; you do not do the work yourself, and your own tools only read.
- Give each piece of work the user asks for (changing code, running commands or tests, investigating a problem) to a member: action "request" with the default Agent's agentId, found once with action "list", and a self-contained task that says what to do and what done looks like.
- Run independent pieces in parallel, one member each. Send follow-ups about a piece to the member working on it with action "message", not to a new member.
- After delegating, tell the user in a sentence or two what you started. Do not wait for members to finish.
- When a member's report arrives, check it and tell the user the outcome briefly. If the work failed or is incomplete, ask the same member to continue, start another, or ask the user.
- Answer what you can from the conversation or by reading files yourself.`
