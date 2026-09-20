import assert from "node:assert/strict"
import childProcess from "node:child_process"
import crypto from "node:crypto"
import { EventEmitter } from "node:events"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { syncBuiltinESMExports } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { PassThrough, Writable } from "node:stream"
import test from "node:test"
import ts from "typescript"

const source = await readFile(new URL("./git-ai.ts", import.meta.url), "utf8")
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
})
// The plugin imports `@opencode/plugin` at runtime, and bare specifiers cannot
// be resolved from a data: URL, so import the transpiled module from a file
// inside the package directory where node_modules resolution works.
const buildUrl = new URL(`./git-ai.test-build-${crypto.randomUUID()}.mjs`, import.meta.url)
await writeFile(buildUrl, outputText)
let plugin
try {
  plugin = await import(buildUrl)
} finally {
  await rm(buildUrl, { force: true })
}

const mockCheckpointSpawn = (t, checkpoints) => {
  t.mock.method(childProcess, "spawn", (command, args) => {
    assert.deepEqual(args, ["checkpoint", "opencode", "--hook-input", "stdin"])
    const child = new EventEmitter()
    child.stderr = new PassThrough()
    child.stdin = new Writable({
      write(chunk, _encoding, done) {
        checkpoints.push(JSON.parse(chunk.toString()))
        done()
      },
      final(done) {
        done()
        queueMicrotask(() => child.emit("close", 0))
      },
    })
    return child
  })
  syncBuiltinESMExports()
}

const makeRepoDir = async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "git-ai-opencode-"))
  t.after(async () => {
    t.mock.restoreAll()
    syncBuiltinESMExports()
    await rm(directory, { recursive: true, force: true })
  })
  await mkdir(join(directory, ".git"))
  return directory
}

test("module exposes a stable display ID and both plugin API entrypoints", () => {
  assert.equal(plugin.default.id, "git-ai")
  assert.equal(plugin.default.setup, plugin.GitAiPlugin.setup)
  assert.equal(typeof plugin.default.server, "function")
  assert.deepEqual(Object.keys(plugin).sort(), ["GitAiPlugin", "GitAiPluginV1", "default"])
})

test("V1 server emits one checkpoint for each edit boundary", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  mockCheckpointSpawn(t, checkpoints)

  const hooks = await plugin.default.server({ directory })
  const input = { tool: "write", callID: "write-1", sessionID: "session-1", args: { filePath: join(directory, "skill.md") } }
  await hooks["tool.execute.before"](input)
  await hooks["tool.execute.after"](input)
  assert.deepEqual(checkpoints.map((event) => event.hook_event_name), ["PreToolUse", "PostToolUse"])
  assert.ok(checkpoints.every((event) => event.session_id === "session-1" && event.tool_use_id === "write-1"))
})

test("V2 setup registers tool hooks that emit one checkpoint per edit boundary", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  mockCheckpointSpawn(t, checkpoints)

  const hooks = {}
  const ctx = {
    location: { directory, project: { directory } },
    tool: {
      hook: async (name, callback) => {
        hooks[name] = callback
        return { dispose: async () => {} }
      },
    },
  }
  await plugin.default.setup(ctx)
  assert.deepEqual(Object.keys(hooks).sort(), ["execute.after", "execute.before"])

  const event = { tool: "write", id: "write-2", sessionID: "session-2", input: { filePath: join(directory, "skill.md") } }
  await hooks["execute.before"](event)
  await hooks["execute.after"]({
    ...event,
    status: "completed",
    result: { metadata: { files: [{ filePath: join(directory, "skill.md") }] } },
  })
  assert.deepEqual(checkpoints.map((item) => item.hook_event_name), ["PreToolUse", "PostToolUse"])
  assert.ok(checkpoints.every((item) => item.session_id === "session-2" && item.tool_use_id === "write-2"))
  assert.deepEqual(checkpoints[1].tool_input.file_paths, [join(directory, "skill.md")])
})

test("V2 setup ignores tools that do not edit files", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  mockCheckpointSpawn(t, checkpoints)

  const hooks = {}
  const ctx = {
    location: { directory, project: { directory } },
    tool: {
      hook: async (name, callback) => {
        hooks[name] = callback
        return { dispose: async () => {} }
      },
    },
  }
  await plugin.default.setup(ctx)

  const event = { tool: "read", id: "read-1", sessionID: "session-3", input: { filePath: join(directory, "skill.md") } }
  await hooks["execute.before"](event)
  await hooks["execute.after"]({ ...event, status: "completed", result: { metadata: {} } })
  assert.deepEqual(checkpoints, [])
})
