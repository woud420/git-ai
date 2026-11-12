import assert from "node:assert/strict"
import { writeFile } from "node:fs/promises"
import { join } from "node:path"
import { pathToFileURL } from "node:url"

const [pluginPath, directory, project] = process.argv.slice(2)
const { default: plugin } = await import(pathToFileURL(pluginPath).href)
const hooks = new Map()
await plugin.setup({
  location: { directory, project: { directory: project } },
  tool: { hook: async (name, callback) => hooks.set(name, callback) },
})
assert.equal(hooks.size, 2)
const event = { tool: "edit", id: "call-1", sessionID: "test-session-123", input: { path: "edit.ts" } }
await hooks.get("execute.before")(event)
await writeFile(join(directory, "edit.ts"), "// baseline\n// AI edit\n")
await hooks.get("execute.after")({ ...event, status: "completed", result: { metadata: {} } })
