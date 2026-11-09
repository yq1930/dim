import type { SessionMember } from "../../../stores/chat"

export type MemberStatus = "running" | "completed" | "failed" | "cancelled"

export function memberStatus(member: SessionMember): MemberStatus {
  return member.finish?.status ?? "running"
}

/** A member is named by its task's first line; the Agent behind it is usually the default one. */
export function memberTask(member: SessionMember): string {
  return member.prompt.split("\n")[0]?.trim() ?? ""
}
