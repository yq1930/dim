/** The Shiki language a file is highlighted as, from its name; a file without one shows plain. */
const BY_EXTENSION: Record<string, string> = {
  bash: "shellscript", c: "c", cc: "cpp", cjs: "javascript", cmake: "cmake", cpp: "cpp", cs: "csharp", css: "css", csv: "csv",
  cts: "typescript", cxx: "cpp", diff: "diff", erl: "erlang", ex: "elixir", exs: "elixir", go: "go", gql: "graphql",
  gradle: "groovy", graphql: "graphql", groovy: "groovy", h: "c", hcl: "hcl", hh: "cpp", hpp: "cpp", hs: "haskell",
  htm: "html", html: "html", ini: "ini", java: "java", js: "javascript", json: "json", json5: "json5", jsonc: "jsonc",
  jsx: "jsx", kt: "kotlin", kts: "kotlin", lua: "lua", markdown: "markdown", md: "markdown", mdx: "markdown", mjs: "javascript",
  mk: "make", mts: "typescript", patch: "diff", php: "php", pl: "perl", plist: "xml", pm: "perl", proto: "proto",
  ps1: "powershell", py: "python", r: "r", rb: "ruby", rs: "rust", scala: "scala", scss: "scss", sh: "shellscript",
  sql: "sql", svelte: "svelte", svg: "xml", swift: "swift", tex: "latex", tf: "hcl", toml: "toml", ts: "typescript",
  tsx: "tsx", vue: "vue", xml: "xml", yaml: "yaml", yml: "yaml", zig: "zig", zsh: "shellscript",
}
const BY_NAME: Record<string, string> = {
  "cargo.lock": "toml", "cmakelists.txt": "cmake", dockerfile: "docker", gnumakefile: "make", makefile: "make",
  ".bashrc": "shellscript", ".zshrc": "shellscript", ".profile": "shellscript",
}

export function fileLanguage(path: string): string | undefined {
  const name = path.slice(path.lastIndexOf("/") + 1).toLowerCase()
  const dot = name.lastIndexOf(".")
  return BY_NAME[name] ?? (dot > 0 ? BY_EXTENSION[name.slice(dot + 1)] : undefined)
}
