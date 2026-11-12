import assert from "node:assert/strict"
import childProcess from "node:child_process"
import { EventEmitter } from "node:events"
import { mkdir, mkdtemp, rm } from "node:fs/promises"
import { syncBuiltinESMExports } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { PassThrough, Writable } from "node:stream"
import test from "node:test"
import { fileURLToPath, pathToFileURL } from "node:url"

// The plugin imports `@opencode/plugin` at runtime, and bare specifiers cannot
// be resolved from a data: URL, so import the compiled module from a file
// inside the package directory where node_modules resolution works.
const buildDirectory = await mkdtemp(new URL("./git-ai.test-build-", import.meta.url))
let plugin
try {
  childProcess.execFileSync(process.execPath, [
    fileURLToPath(new URL("./node_modules/typescript/bin/tsc", import.meta.url)),
    "--noEmit", "false", "--outDir", buildDirectory,
  ], {
    cwd: fileURLToPath(new URL(".", import.meta.url)),
    stdio: "inherit",
    timeout: 30_000,
  })
  plugin = await import(pathToFileURL(join(buildDirectory, "git-ai.js")))
} finally {
  await rm(buildDirectory, { recursive: true, force: true })
}

const mockCheckpointSpawn = (t, checkpoints, close = (child) => child.emit("close", 0)) => {
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
        queueMicrotask(() => close(child))
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


async function resourceFixture(t, behavior = (child) => child.emit("close", 0)) {
  const directory = await makeRepoDir(t)
  const calls = []
  const children = []
  const warnings = []
  t.mock.method(console, "error", (...args) => warnings.push(args.join(" ")))
  t.mock.method(childProcess, "spawn", () => {
    const child = new EventEmitter()
    child.stderr = new EventEmitter()
    child.stdin = new EventEmitter()
    child.stdin.destroy = () => { child.stdin.destroyed = true }
    child.stderr.destroy = () => { child.stderr.destroyed = true }
    child.stdin.end = (input) => { calls.push(JSON.parse(input)); queueMicrotask(() => behavior(child)) }
    child.signals = []
    child.kill = (signal) => { child.signals.push(signal); return true }
    children.push(child)
    return child
  })
  syncBuiltinESMExports()
  const hooks = {}
  const cleanup = await plugin.default.setup({
    location: { directory },
    tool: { hook: async (name, callback) => { hooks[name] = callback; return { dispose: async () => {} } } },
  })
  const event = (id = "call", input = { path: "a.ts" }) => ({ id, sessionID: "session", tool: "edit", input, status: "completed" })
  return { before: hooks["execute.before"], after: hooks["execute.after"], cleanup, calls, children, warnings, event }
}

test("capacity preserves accepted calls and admits new calls after completion", async (t) => {
  const f = await resourceFixture(t)
  for (let i = 0; i < 65; i++) await f.before(f.event(`call-${i}`))
  assert.equal(f.calls.length, 64)
  assert.ok(f.warnings.some((line) => /capacity|limit/i.test(line)))
  await f.after(f.event("call-0"))
  await f.before(f.event("replacement"))
  await f.after(f.event("replacement"))
  assert.equal(f.calls.length, 67)
})

test("shell input and total pending bytes are bounded", async (t) => {
  const f = await resourceFixture(t)
  const command = "x".repeat(600 * 1024)
  for (let i = 0; i < 20; i++) await f.before({ ...f.event(`large-${i}`, { cmd: command, unused: "y".repeat(100) }), tool: "shell" })
  assert.equal(f.calls.length, 13)
  assert.deepEqual(f.calls[0].tool_input, { command })
  await f.after({ ...f.event("large-0"), tool: "shell" })
  assert.equal(f.calls.at(-1).tool_input.command, command)
  await f.before({ ...f.event("oversized", { command: "x".repeat(1024 * 1024 + 1) }), tool: "shell" })
  assert.equal(f.calls.length, 14)
})

test("disposal during an in-flight pre checkpoint prevents a later post", async (t) => {
  const f = await resourceFixture(t, () => {})
  const pre = f.before(f.event())
  while (!f.children.length) await new Promise(setImmediate)
  assert.equal(typeof f.cleanup, "function")
  f.cleanup()
  assert.deepEqual(f.children[0].signals, ["SIGTERM"])
  f.children[0].emit("close", 0)
  await pre
  await f.after(f.event())
  assert.equal(f.calls.length, 1)
})

test("stderr retention is bounded for a large chunk", async (t) => {
  const previous = process.env.GIT_AI_OPENCODE_DEBUG
  process.env.GIT_AI_OPENCODE_DEBUG = "1"
  t.after(() => { if (previous === undefined) delete process.env.GIT_AI_OPENCODE_DEBUG; else process.env.GIT_AI_OPENCODE_DEBUG = previous })
  const f = await resourceFixture(t, (child) => { child.stderr.emit("data", Buffer.alloc(4 * 1024 * 1024, "x")); child.emit("close", 23) })
  await f.before(f.event())
  assert.ok(f.warnings.join("").length < 66 * 1024)
  assert.match(f.warnings.join(""), /truncat/i)
})

test("checkpoint timeout escalates and closes inherited pipes", async (t) => {
  const f = await resourceFixture(t, () => {})
  t.mock.timers.enable({ apis: ["setTimeout"] })
  const pre = f.before(f.event())
  while (!f.children.length) await new Promise(setImmediate)
  t.mock.timers.tick(10_000)
  assert.deepEqual(f.children[0].signals, ["SIGTERM"])
  t.mock.timers.tick(250)
  await pre
  assert.deepEqual(f.children[0].signals, ["SIGTERM", "SIGKILL"])
  assert.equal(f.children[0].stderr.destroyed, true)
  assert.equal(f.children[0].stdin.destroyed, true)
})

test("accepted tool calls do not expire while tools run", async (t) => {
  const f = await resourceFixture(t)
  t.mock.timers.enable({ apis: ["setTimeout"] })
  await f.before(f.event())
  t.mock.timers.tick(24 * 60 * 60 * 1000)
  await f.after(f.event())
  assert.equal(f.calls.length, 2)
})


test("disposal closes descendant-held pipes without signaling a process group", async (t) => {
  const f = await resourceFixture(t, (child) => child.emit("exit", 0))
  const groupSignals = []
  t.mock.method(process, "kill", (...args) => { groupSignals.push(args); return true })
  t.mock.timers.enable({ apis: ["setTimeout"] })
  const pre = f.before(f.event())
  while (!f.children.length) await new Promise(setImmediate)
  f.children[0].pid = 12345
  f.cleanup()
  t.mock.timers.tick(250)
  await pre
  assert.deepEqual(f.children[0].signals, ["SIGTERM", "SIGKILL"])
  assert.deepEqual(groupSignals, [])
  assert.equal(f.children[0].stderr.destroyed, true)
  assert.equal(f.children[0].stdin.destroyed, true)
})


test("oversized edit paths are rejected and post metadata cannot expand scope", async (t) => {
  const f = await resourceFixture(t)
  await f.before(f.event("large-path", { path: "x".repeat(1024 * 1024 + 1) }))
  assert.equal(f.calls.length, 0)
  const event = f.event("metadata")
  await f.before(event)
  await f.after({ ...event, status: "completed", result: { metadata: { files: [{ path: "x".repeat(1024 * 1024 + 1) }] } } })
  assert.equal(f.calls.length, 2)
  assert.deepEqual(f.calls[1].tool_input, f.calls[0].tool_input)
  await f.before(f.event("replacement"))
  assert.equal(f.calls.length, 3)
})


test("shell post checkpoints ignore metadata paths", async (t) => {
  const f = await resourceFixture(t)
  const event = { ...f.event("shell", { command: "touch a.ts" }), tool: "shell" }
  await f.before(event)
  await f.after({ ...event, status: "completed", result: { metadata: { files: [{ path: "a.ts" }] } } })
  assert.equal(f.calls[1].tool_input.command, "touch a.ts")
  assert.equal(f.calls[1].tool_input.file_paths, undefined)
})

test("V1 isolates calls with the same ID in different sessions", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  mockCheckpointSpawn(t, checkpoints)
  const hooks = await plugin.default.server({ directory })
  const first = { tool: "write", callID: "same", sessionID: "first", args: { path: "first.txt" } }
  const second = { ...first, sessionID: "second", args: { path: "second.txt" } }
  await hooks["tool.execute.before"](first)
  await hooks["tool.execute.before"](second)
  await hooks["tool.execute.after"](first)
  await hooks["tool.execute.after"](second)
  const posts = checkpoints.filter((c) => c.hook_event_name === "PostToolUse")
  assert.deepEqual(posts.map((c) => [c.session_id, c.tool_input.file_paths[0]]), [
    ["first", join(directory, "first.txt")], ["second", join(directory, "second.txt")],
  ])
})

test("failed and repeated pre-checkpoints cannot reuse an older baseline", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  let count = 0
  mockCheckpointSpawn(t, checkpoints, (child) => child.emit("close", ++count === 2 ? 1 : 0))
  const hooks = await plugin.default.server({ directory })
  const input = { tool: "write", callID: "same", sessionID: "session", args: { path: "a.txt" } }
  await hooks["tool.execute.before"](input)
  await hooks["tool.execute.before"](input)
  await hooks["tool.execute.after"](input)
  assert.deepEqual(checkpoints.map((c) => c.hook_event_name), ["PreToolUse", "PreToolUse"])
})

test("V1 terminal events cancel matching calls or a deleted session", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  mockCheckpointSpawn(t, checkpoints)
  const hooks = await plugin.default.server({ directory })
  const input = { tool: "write", callID: "same", sessionID: "session", args: { path: "a.txt" } }
  await hooks["tool.execute.before"](input)
  await hooks.event({ event: { type: "message.part.updated", properties: { part: { type: "tool", sessionID: "session", callID: "same", state: { status: "error" } } } } })
  await hooks["tool.execute.after"](input)
  await hooks["tool.execute.before"](input)
  await hooks.event({ event: { type: "session.deleted", properties: { info: { id: "session" } } } })
  await hooks["tool.execute.after"](input)
  assert.deepEqual(checkpoints.map((c) => c.hook_event_name), ["PreToolUse", "PreToolUse"])
})

test("V2 rejects failed results and mismatched messages and ignores result paths", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  mockCheckpointSpawn(t, checkpoints)
  const hooks = {}
  await plugin.default.setup({ location: { directory }, tool: { hook: async (name, callback) => { hooks[name] = callback } } })
  const event = { tool: "write", id: "call", sessionID: "session", messageID: "message", input: { path: "a.txt" } }
  await hooks["execute.before"](event)
  await hooks["execute.after"]({ ...event, messageID: "other", status: "completed" })
  await hooks["execute.after"]({ ...event, status: "failed" })
  await hooks["execute.after"]({ ...event, status: "completed" })
  await hooks["execute.before"](event)
  await hooks["execute.after"]({ ...event, status: "completed", result: { metadata: { files: [{ path: "extra.txt" }] } } })
  assert.deepEqual(checkpoints.map((c) => c.hook_event_name), ["PreToolUse", "PreToolUse", "PostToolUse"])
  assert.deepEqual(checkpoints.at(-1).tool_input, { file_paths: [join(directory, "a.txt")] })
})

test("cancellation during a pre-checkpoint cannot restore the call", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  let child
  mockCheckpointSpawn(t, checkpoints, (pending) => { child = pending })
  const hooks = await plugin.default.server({ directory })
  const input = { tool: "write", callID: "call", sessionID: "session", args: { path: "a.txt" } }
  const pre = hooks["tool.execute.before"](input)
  while (!child) await new Promise(setImmediate)
  await hooks.event({ event: { type: "session.deleted", properties: { info: { id: "session" } } } })
  child.emit("close", 0)
  await pre
  await hooks["tool.execute.after"](input)
  assert.deepEqual(checkpoints.map((c) => c.hook_event_name), ["PreToolUse"])
})

test("V2 lifecycle events cancel only matching messages and sessions", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  mockCheckpointSpawn(t, checkpoints)
  const hooks = {}
  let deliver
  const event = {
    subscribe: async function* ({ signal }) {
      while (!signal.aborted) {
        const value = await new Promise((resolve) => {
          deliver = resolve
          signal.addEventListener("abort", () => resolve(undefined), { once: true })
        })
        if (value) yield value
      }
    },
  }
  await plugin.default.setup({ location: { directory }, event, tool: { hook: async (name, callback) => { hooks[name] = callback } } })
  const call = { tool: "write", id: "call", sessionID: "session", messageID: "message", input: { path: "a.txt" } }
  const emit = async (type, data) => {
    deliver({ type, data })
    await new Promise(setImmediate)
  }
  await hooks["execute.before"](call)
  await emit("session.tool.failed", { sessionID: "session", id: "call", assistantMessageID: "other" })
  await hooks["execute.after"]({ ...call, status: "completed" })
  await hooks["execute.before"](call)
  await emit("session.tool.failed", { sessionID: "session", id: "call", assistantMessageID: "message" })
  await hooks["execute.after"]({ ...call, status: "completed" })
  await hooks["execute.before"](call)
  await emit("session.deleted", { sessionID: "session" })
  await hooks["execute.after"]({ ...call, status: "completed" })
  await hooks["execute.before"](call)
  await emit("session.tool.success", { sessionID: "session", id: "call", assistantMessageID: "message" })
  await hooks["execute.after"]({ ...call, status: "completed" })
  assert.deepEqual(checkpoints.map((c) => c.hook_event_name), ["PreToolUse", "PostToolUse", "PreToolUse", "PreToolUse", "PreToolUse"])
})

test("post-checkpoint uses the input captured before execution", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  mockCheckpointSpawn(t, checkpoints)
  const hooks = await plugin.default.server({ directory })
  const input = { tool: "write", callID: "call", sessionID: "session", args: { files: ["a.txt"] } }
  await hooks["tool.execute.before"](input)
  input.args.files.push("extra.txt")
  await hooks["tool.execute.after"](input)
  assert.deepEqual(checkpoints[1].tool_input.file_paths, [join(directory, "a.txt")])
})

test("repeated pre while a checkpoint is running cannot reuse its baseline", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  let child
  mockCheckpointSpawn(t, checkpoints, (pending) => { child = pending })
  const hooks = await plugin.default.server({ directory })
  const input = { tool: "write", callID: "call", sessionID: "session", args: { path: "a.txt" } }
  const first = hooks["tool.execute.before"](input)
  while (!child) await new Promise(setImmediate)
  await hooks["tool.execute.before"](input)
  await hooks["tool.execute.before"](input)
  child.emit("close", 0)
  await first
  await hooks["tool.execute.after"](input)
  assert.deepEqual(checkpoints.map((c) => c.hook_event_name), ["PreToolUse"])
})

test("a failed pre-checkpoint cannot authorize a post-checkpoint", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  mockCheckpointSpawn(t, checkpoints, (child) => child.emit("close", 1))
  const hooks = await plugin.default.server({ directory })
  const input = { tool: "write", callID: "call", sessionID: "session", args: { path: "a.txt" } }
  await hooks["tool.execute.before"](input)
  await hooks["tool.execute.after"](input)
  assert.deepEqual(checkpoints.map((c) => c.hook_event_name), ["PreToolUse"])
})


test("disposal closes lifecycle subscription without reopening it for remaining calls", async (t) => {
  const directory = await makeRepoDir(t)
  const checkpoints = []
  mockCheckpointSpawn(t, checkpoints)
  const hooks = {}
  const signals = []
  const event = {
    subscribe: async function* ({ signal }) {
      signals.push(signal)
      await new Promise((resolve) => signal.addEventListener("abort", resolve, { once: true }))
    },
  }
  const cleanup = await plugin.default.setup({
    location: { directory }, event,
    tool: { hook: async (name, callback) => { hooks[name] = callback } },
  })
  const call = { tool: "write", id: "first", sessionID: "session", input: { path: "a.txt" } }
  await hooks["execute.before"](call)
  await hooks["execute.before"]({ ...call, id: "second" })
  cleanup()
  await new Promise(setImmediate)
  assert.equal(signals.length, 1)
  assert.equal(signals[0].aborted, true)
  await hooks["execute.after"]({ ...call, status: "completed" })
  assert.equal(checkpoints.length, 2)
})
