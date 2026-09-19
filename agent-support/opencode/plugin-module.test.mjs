import assert from "node:assert/strict"
import childProcess from "node:child_process"
import { EventEmitter } from "node:events"
import { mkdir, mkdtemp, readFile, rm } from "node:fs/promises"
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
const plugin = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`)

test("module exposes a stable display ID and the legacy server entrypoint", () => {
  assert.equal(plugin.default.id, "git-ai")
  assert.equal(plugin.default.server, plugin.GitAiPlugin)
  assert.deepEqual(Object.keys(plugin).sort(), ["GitAiPlugin", "default"])
})

test("module server emits one checkpoint for each edit boundary", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "git-ai-opencode-"))
  t.after(async () => {
    t.mock.restoreAll()
    syncBuiltinESMExports()
    await rm(directory, { recursive: true, force: true })
  })
  await mkdir(join(directory, ".git"))
  const checkpoints = []
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

  const hooks = await plugin.default.server({ directory })
  const input = { tool: "write", callID: "write-1", sessionID: "session-1", args: { filePath: join(directory, "skill.md") } }
  await hooks["tool.execute.before"](input)
  await hooks["tool.execute.after"](input)
  assert.deepEqual(checkpoints.map((event) => event.hook_event_name), ["PreToolUse", "PostToolUse"])
  assert.ok(checkpoints.every((event) => event.session_id === "session-1" && event.tool_use_id === "write-1"))
})
