---
name: init
description: Create or update the project's AGENTS.md from what the repository actually contains. Use when asked to initialize a project for Dim, or to write or refresh AGENTS.md.
---

# Initialize AGENTS.md

`AGENTS.md` at the workspace root is loaded into every Session in this project, so every line
costs context on every request. Write only what an agent could not work out quickly from the
repository itself.

## Steps

1. Look for instructions that already exist: `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.cursorrules`,
   `.cursor/rules/`, `.github/copilot-instructions.md`, `CONTRIBUTING.md` and the README.
2. Learn how the project is built and laid out from its manifests, lockfiles, CI configuration and
   test directories. Read only; this task changes no file but `AGENTS.md`.
3. If `AGENTS.md` exists, edit it in place: keep what is still true, correct what is not, and ask
   the user before removing a rule you cannot verify. Otherwise create it at the workspace root.
4. Tell the user what you wrote, and name anything you could not verify.

## What to write

- **Commands.** The exact commands to build, run, test, lint and format, including how to run a
  single test. Take them from the manifests and CI, not from what is usual for the stack.
- **Architecture that spans files.** Entry points, how the main parts connect, and where a change of
  a given kind belongs: what takes reading several files to learn.
- **Conventions the tools do not enforce.** Rules that differ from the language's defaults, that a
  formatter or linter would not catch, and that the code shows consistently.
- **Traps.** Generated files not to edit, required environment setup, and steps that must happen in
  order. Name the variable, never its value.

## What to leave out

- A listing of files and directories an agent can find by looking.
- General advice, such as writing tests or handling errors.
- Secrets, credentials and machine-specific paths.
- Anything you only assume. If the repository does not show it, leave it out or ask.

Aim for well under 150 lines. Prefer a short file that is entirely true.
