/**
 * The draft after quoting `message` into it: the message as a Markdown quote
 * and a blank line, after what the draft holds. The TUI composer writes the
 * same format (member-conversation design); `quote-cases.json` pins it for both.
 */
export function quoteInto(draft: string, message: string): string {
  let text = message.replaceAll("\r\n", "\n")
  if (text.endsWith("\n")) text = text.slice(0, -1)
  if (text === "") return draft
  const block = text.split("\n").map((line) => (line === "" ? ">\n" : `> ${line}\n`)).join("")
  const gap = draft === "" || draft.endsWith("\n\n") ? "" : draft.endsWith("\n") ? "\n" : "\n\n"
  return `${draft}${gap}${block}\n`
}
