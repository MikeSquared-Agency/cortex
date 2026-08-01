import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import test from "node:test";

function request(child, message) {
  return new Promise((resolve, reject) => {
    const onData = (chunk) => {
      try {
        resolve(JSON.parse(chunk.toString().trim()));
      } catch (error) {
        reject(error);
      }
    };
    child.stdout.once("data", onData);
    child.stdin.write(`${JSON.stringify(message)}\n`);
  });
}

test("initializes and exposes the Cortex tools", async (t) => {
  const child = spawn(process.execPath, ["cortex-mcp-bridge.js"], {
    cwd: import.meta.dirname,
    stdio: ["pipe", "pipe", "pipe"],
  });

  t.after(async () => {
    child.kill();
    await once(child, "exit");
  });

  const initialized = await request(child, {
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {},
  });
  assert.equal(initialized.result.serverInfo.name, "cortex-mcp-bridge");
  assert.equal(initialized.result.serverInfo.version, "0.3.2");

  const tools = await request(child, {
    jsonrpc: "2.0",
    id: 2,
    method: "tools/list",
    params: {},
  });
  assert.deepEqual(
    tools.result.tools.map((tool) => tool.name),
    [
      "cortex_store",
      "cortex_search",
      "cortex_recall",
      "cortex_briefing",
      "cortex_traverse",
      "cortex_relate",
      "cortex_observe",
    ],
  );
});
