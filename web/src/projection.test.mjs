import assert from "node:assert/strict"
import { registerHooks } from "node:module"
import test from "node:test"

registerHooks({
  resolve(specifier, context, next) {
    try {
      return next(specifier, context)
    } catch (error) {
      if ((specifier.startsWith("./") || specifier.startsWith("../")) && !specifier.endsWith(".ts")) {
        return next(`${specifier}.ts`, context)
      }
      throw error
    }
  },
})

const { Projection } = await import("./projection.ts")

function event(sequence, content) {
  return { id: `e${sequence}`, sequence, createAt: 1_000 + sequence, content }
}

function input(text, extra = {}) {
  return { kind: "input", text, user: { content: [{ type: "text", text }] }, blocks: [], ...extra }
}

test("a user message carries the cut a child line opens at", () => {
  const projection = new Projection()
  projection.apply(event(5, input("try RwLock\nfor the cache lock")))
  projection.apply(event(6, { kind: "input-applied", inputId: "e5" }))
  projection.apply(event(7, {
    kind: "branch-create",
    branchId: "b-7c",
    at: 4,
    prompt: "parser race",
    source: { kind: "host", operationId: "op" },
  }))

  const [user, line] = projection.rows
  assert.equal(user.kind, "user")
  assert.equal(user.sequence, 5)
  assert.equal(user.cut, 4)
  assert.equal(line.kind, "line")
  assert.equal(line.branchId, "b-7c")
  assert.equal(line.purpose, "parser race")
  assert.equal(line.before, "try RwLock")
  assert.equal(line.failed, undefined)

  projection.apply(event(8, {
    kind: "branch-create-failed",
    branchId: "b-7c",
    status: "failed",
    error: "boom",
  }))
  assert.equal(projection.rows[1].failed, true)
})

test("branch messages, ends and the last turn end become rows", () => {
  const projection = new Projection()
  projection.apply(event(1, {
    kind: "source-input",
    text: "<dim-branch-message from=\"b-7c\" to=\"main\">\nStop &amp;&amp; look at parser.rs.\n</dim-branch-message>",
    blocks: [],
    mode: "steer",
  }))
  projection.apply(event(2, {
    kind: "branch-end",
    source: { kind: "host", operationId: "op" },
  }))
  projection.apply(event(3, { kind: "turn-end", outcome: "completed" }))

  const [message, ended] = projection.rows
  assert.deepEqual(message, {
    kind: "source-message",
    key: "notice-1",
    from: "b-7c",
    steer: true,
    final: false,
    body: "Stop && look at parser.rs.",
  })
  assert.equal(ended.kind, "line-ended")
  assert.equal(ended.issuer, "operation")
  assert.equal(projection.lastTurnEnd, 3)
})

test("replayed events do not duplicate rows", () => {
  const projection = new Projection()
  const written = event(1, input("hello"))
  const applied = event(2, { kind: "input-applied", inputId: "e1" })
  assert.equal(projection.apply(written), true)
  assert.equal(projection.apply(applied), true)
  assert.equal(projection.apply(written), false)
  assert.equal(projection.apply(applied), false)
  assert.equal(projection.rows.length, 1)
})

test("a user row carries the Skills its message loaded", () => {
  const projection = new Projection()
  projection.apply(event(5, input("check the build", { skills: ["checkin"] })))
  projection.apply(event(6, { kind: "input-applied", inputId: "e5" }))
  assert.deepEqual(projection.rows, [{
    kind: "user",
    key: "user-5",
    text: "check the build",
    skills: ["checkin"],
    at: 1_005,
    sequence: 5,
    cut: 4,
  }])
})
