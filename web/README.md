# Web workspace frontend

The browser frontend that `dimcode web` opens: a Vue 3 + Vite app that speaks App Server protocol v3 (JSON-RPC over the web entry's `/ws`). The design is [docs/design/web-workspace.md](../docs/design/web-workspace.md) §4.

Use the Node version in [`.node-version`](../.node-version) and npm.

## Develop against a running entry

1. Open the web entry from the CLI: `dimcode web --no-open`. It prints the address (default `http://127.0.0.1:43201/`); the link it would open carries `#bootstrap=<token>`.
2. Start the dev server, which forwards `/api` and `/ws` to the entry:

   ```sh
   cd web
   npm ci
   npm run dev                                   # entry on 127.0.0.1:43201
   DIM_WEB_TARGET=http://127.0.0.1:43202 npm run dev   # another port
   ```

3. Open `http://localhost:5173/#bootstrap=<token>` with the token from the CLI's link. The page exchanges it for the access cookie and drops it from the address bar. A page that says the link expired needs a new token (run `dimcode web` again).

The proxy rewrites `Origin` to the entry's own address, because the entry accepts only same-origin requests.

## Build

```sh
cd web
npm ci
npm run build      # vue-tsc type check, then vite build into web/dist
npm run typecheck  # type check only
```

`web/dist` is what the entry serves: `index.html` for every non-asset path and `/assets/*` as immutable files. The build keeps to the entry's CSP: no inline scripts, no `data:` fonts, and Shiki highlights with its JavaScript regex engine (WebAssembly would need `wasm-unsafe-eval`). `vite.config.ts` replaces markstream's optional `@terrastruct/d2` renderer with an inert module; that renderer is intentionally not shipped, so removing the alias pulls it into the bundle.

## Layout

The right detail panel shows the Session in tabs: Changes, Plan, Background
tasks, Terminal and Lines; the Session header's buttons open them. The
**Background tasks** tab (also `/tasks`) lists background commands across the
Session's lines. Select a running command to see its live output tail; **Stop
command** asks for confirmation before stopping that command. Leaving the tab
leaves commands running. The **Terminal** tab runs shells in the Session's
workspace through the App Server's `terminal.*` methods; Ctrl+` shows or hides it. The
**Git** tab shows the Session's branch, its pull request and CI checks
(`git.*` methods, with the GitHub CLI); a new Session in a Git repository can
start in a worktree of its own. Reopening the Session rebuilds the
list from recorded history. Failed history reads and unavailable live output are
shown explicitly, and output is rendered as text.

| Path | Purpose |
| --- | --- |
| `src/rpc/client.ts` | WebSocket JSON-RPC client: `hello` v3, timeouts, `daemon.ping` heartbeat, reconnect with backoff, bootstrap exchange |
| `src/rpc/protocol.ts`, `src/rpc/read.ts` | Protocol types and the strict readers that decode every reply and notification |
| `src/projection.ts` | Folds history pages, `session.event` and `session.model.progress` into timeline rows (as `crates/dim-cli/src/projection.rs`) |
| `src/lines.ts` | Folds branch facts into the Session's line tree, line states and cut message counts |
| `src/stores/` | Connection, session directory, attached sessions and their per-line streams, models, layout, each Session's panes and browser preferences |
| `src/app/` | The shell, the keyboard shortcut table and its dialog, keeping the computer awake and auto-archive |
| `src/components/` | Controls shared across regions: dialogs, menus, toasts, tooltips, Markdown, diffs and the workspace file finder |
| `src/features/sidebar/` | Workspaces and sessions, their states and actions |
| `src/features/session/` | The session and new-session pages; `header/` (pane buttons, open in, transcript view, outline, branch), `transcript/` (the timeline) and `composer/` (the composer, its menus and the prompts that take its place) |
| `src/features/panes/` | The right panel, one folder per view: changes, files, preview, plan, tasks, terminal (xterm.js views of the App Server's terminals), git and lines |
| `src/features/settings/`, `src/features/schedules/` | Settings pages with their registry and search, and scheduled tasks |
