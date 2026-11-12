import assert from "node:assert/strict"
import childProcess from "node:child_process"
import { EventEmitter } from "node:events"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { syncBuiltinESMExports } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { PassThrough, Writable } from "node:stream"
import test from "node:test"
import { fileURLToPath, pathToFileURL } from "node:url"

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

test("installed plugin loads without node_modules and registers both APIs", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "git-ai-opencode-installed-"))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const pluginDirectory = join(directory, ".config", "opencode", "plugins")
  await mkdir(pluginDirectory, { recursive: true })
  const source = await readFile(new URL("./git-ai.ts", import.meta.url), "utf8")
  const installedSource = source.replace("__GIT_AI_BINARY_PATH__", "/usr/bin/git-ai")
  await writeFile(join(pluginDirectory, "git-ai.ts"), installedSource)
  childProcess.execFileSync(process.execPath, [
    fileURLToPath(new URL("./node_modules/typescript/bin/tsc", import.meta.url)),
    "--ignoreConfig", "--noCheck", "--module", "ESNext", "--target", "ES2022",
    "--outDir", pluginDirectory, join(pluginDirectory, "git-ai.ts"),
  ], { timeout: 30_000 })
  const pluginPath = join(pluginDirectory, "git-ai.mjs")
  await writeFile(pluginPath, await readFile(join(pluginDirectory, "git-ai.js")))
  const { default: installedPlugin } = await import(pathToFileURL(pluginPath).href)

  assert.equal(installedPlugin.id, "git-ai")
  const hooks = []
  await installedPlugin.setup({
    location: { directory },
    tool: { hook: async (name) => hooks.push(name) },
  })
  assert.deepEqual(hooks, ["execute.before", "execute.after"])
  const legacyHooks = await installedPlugin.server({ directory })
  assert.equal(typeof legacyHooks["tool.execute.before"], "function")
  assert.equal(typeof legacyHooks["tool.execute.after"], "function")
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
  const event = (id = "call", input = { path: "a.ts" }) => ({ id, sessionID: "session", tool: "edit", input })
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


test("oversized edit paths and post metadata do not bypass the byte limit", async (t) => {
  const f = await resourceFixture(t)
  await f.before(f.event("large-path", { path: "x".repeat(1024 * 1024 + 1) }))
  assert.equal(f.calls.length, 0)
  const event = f.event("metadata")
  await f.before(event)
  await f.after({ ...event, status: "completed", result: { metadata: { files: [{ path: "x".repeat(1024 * 1024 + 1) }] } } })
  assert.equal(f.calls.length, 1)
  await f.before(f.event("replacement"))
  assert.equal(f.calls.length, 2)
})


test("shell post checkpoints retain bounded metadata paths", async (t) => {
  const f = await resourceFixture(t)
  const event = { ...f.event("shell", { command: "touch a.ts" }), tool: "shell" }
  await f.before(event)
  await f.after({ ...event, status: "completed", result: { metadata: { files: [{ path: "a.ts" }] } } })
  assert.equal(f.calls[1].tool_input.command, "touch a.ts")
  assert.ok(f.calls[1].tool_input.file_paths[0].endsWith("a.ts"))
})
