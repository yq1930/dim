import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"
import { quoteInto } from "./src/features/session/composer/quote.ts"

const cases = JSON.parse(readFileSync(new URL("./src/features/session/composer/quote-cases.json", import.meta.url), "utf8"))

for (const { name, draft, message, result } of cases) {
  test(name, () => assert.equal(quoteInto(draft, message), result))
}
