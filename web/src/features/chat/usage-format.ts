/** A token count as the TUI's composer border words it: `1.8K`, `256K`, `1M`. */
export function formatTokens(tokens: number): string {
  if (tokens >= 1_000_000) return `${Math.floor(tokens / 100_000) / 10}M`
  if (tokens >= 1000) return `${Math.floor(tokens / 100) / 10}K`
  return String(Math.floor(tokens))
}
