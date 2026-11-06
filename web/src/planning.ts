/**
 * A branch's task list and plan mode, folded from the keyed blocks Planning
 * writes (docs/design/plugins/Planning.md §3): the latest live block of each
 * key decides, a retired one clears it, and a branch start restarts the fold.
 */
import type { RawBlock, SessionEvent, TodoItem } from "./rpc/protocol"

const TODO_KEY = "todo:list"
const PLAN_KEY = "plan:mode"
const MARKS: Record<string, TodoItem["status"]> = { "[x]": "completed", "[>]": "in_progress", "[ ]": "pending" }

function unescape(text: string): string {
  return text.replaceAll("&quot;", "\"").replaceAll("&lt;", "<").replaceAll("&gt;", ">").replaceAll("&amp;", "&")
}

export class PlanningFold {
  private todoBlock: string | undefined
  private planBlock: string | undefined

  apply(event: SessionEvent): void {
    const content = event.content
    switch (content.kind) {
      case "branch-start":
        this.todoBlock = undefined
        this.planBlock = undefined
        break
      case "input":
      case "source-input":
        for (const block of content.blocks) this.add(block)
        break
    }
  }

  /** Whether plan mode is on for this branch. */
  get planning(): boolean {
    return this.planBlock?.startsWith("<dim-plan status=\"active\"") === true
  }

  /** The branch's task list, in order. */
  todos(): TodoItem[] {
    if (this.todoBlock === undefined) return []
    return this.todoBlock.split("\n").slice(1, -1).flatMap((line) => {
      const status = MARKS[line.slice(0, 3)]
      return status === undefined ? [] : [{ status, content: unescape(line.slice(4)) }]
    })
  }

  private add(block: RawBlock): void {
    if (block.key === TODO_KEY) this.todoBlock = block.retire ? undefined : block.text
    else if (block.key === PLAN_KEY) this.planBlock = block.retire ? undefined : block.text
  }
}
