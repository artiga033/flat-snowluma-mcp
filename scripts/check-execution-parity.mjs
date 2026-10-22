#!/usr/bin/env node
// Run both MCP servers against local HTTP fixtures; never contact a real bot.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const { values } = parseArgs({ options: {
  binary: { type: "string" }, "official-server": { type: "string" },
  help: { type: "boolean", short: "h" },
} });
if (values.help) {
  console.log("Usage: node scripts/check-execution-parity.mjs --binary <MCP binary> [--official-server <official server.js>]");
  process.exit(0);
}
assert.ok(values.binary, "--binary is required");
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const catalog = JSON.parse(await readFile(join(root, "assets/catalog.json"), "utf8"));
const temporary = await mkdtemp(join(tmpdir(), "flat-parity-"));
const uploads = new Map();
const requests = [];
const fixtureErrors = [];
let scenario = "ordinary";
const ok = (data) => ({ status: "ok", retcode: 0, data });
const fileFrames = () => [
  ok({ type: "stream", data_type: "file_info", file_name: "example.bin", file_size: 5, chunk_size: 65536 }),
  ok({ type: "stream", data_type: "file_chunk", index: 0, size: 5, data: "aGVsbG8=" }),
  ok({ type: "response", data_type: "file_complete", total_bytes: 5, total_chunks: 1 }),
];

const server = createServer(async (req, res) => {
  try {
    let body = "";
    for await (const bytes of req) body += bytes;
    const request = JSON.parse(body);
    requests.push(request);
    assert.deepEqual(Object.keys(request).sort(), ["action", "params"]);
    assert.equal(request.params.execution, undefined);
    const { action, params } = request;
    res.setHeader("Content-Type", "application/json");
    let frames;
    if (action === "upload_file_stream") {
      if (params.reset) {
        uploads.delete(params.stream_id);
        frames = [{ status: "failed", retcode: 100, data: null, wording: "Stream reset completed" }];
      } else if (params.is_complete) {
        const bytes = Buffer.concat(uploads.get(params.stream_id) ?? []);
        const sha256 = createHash("sha256").update(bytes).digest("hex");
        assert.equal(params.expected_sha256, sha256);
        assert.equal(params.file_size, bytes.length);
        frames = [ok({ type: "response", status: "file_complete", file_path: "/bot/report.bin", file_size: bytes.length, sha256 })];
      } else {
        const chunks = uploads.get(params.stream_id) ?? [];
        chunks.push(Buffer.from(params.chunk_data, "base64"));
        uploads.set(params.stream_id, chunks);
        frames = [ok({ type: "stream", status: "chunk_received" })];
      }
    } else if (action.startsWith("download_file")) {
      frames = fileFrames();
      if (scenario === "unknown-size") frames[0].data.file_size = 0;
      if (scenario === "bad-index") frames[1].data.index = 1;
      if (scenario === "missing-total") delete frames[2].data.total_bytes;
      if (scenario === "bad-terminal") { frames[2].status = "failed"; frames[2].retcode = 100; }
      if (scenario === "trailing") frames.push(ok({ type: "response" }));
      if (scenario === "missing-retcode") delete frames[2].retcode;
    } else if (action === "test_download_stream") {
      frames = [ok({ type: "stream", data: "part" }), ok({ type: "stream", data: "part2" }), ok({ type: "response", data: "done" })];
      if (scenario === "too-many") frames = Array.from({ length: 257 }, (_, i) => ok({ type: i === 256 ? "response" : "stream", data: "x" }));
      if (scenario === "too-much-text") frames[0].data.data = "x".repeat(256 * 1024);
      if (scenario === "long-healthy") frames = Array.from({ length: 8 }, (_, i) => ok({ type: i === 7 ? "response" : "stream", data: "x" }));
    } else {
      if (scenario === "http-failure") res.statusCode = 502;
      const result = ["logical-failure", "http-failure"].includes(scenario)
        ? { status: "failed", retcode: 100, data: null, wording: "fixture failure" }
        : ok({ user_id: 123, nickname: "fixture" });
      res.end(JSON.stringify(result));
      return;
    }
    const delay = scenario === "long-healthy" ? 40 : scenario === "idle" ? 400 : 0;
    for (const frame of frames) {
      if (res.destroyed) break;
      const raw = scenario === "decimal-integers"
        ? JSON.stringify(frame).replace(/:(\d+)(?=[,}])/g, ":$1.0")
        : JSON.stringify(frame);
      res.write(`${raw}\r\n\r\n`);
      if (delay) await new Promise((done) => setTimeout(done, delay));
    }
    res.end();
  } catch (error) {
    fixtureErrors.push(error.message);
    res.destroy(error);
  }
});
server.listen(0, "127.0.0.1");
await once(server, "listening");
const endpoint = `http://127.0.0.1:${server.address().port}/`;

async function client(command, args, directory) {
  await mkdir(directory, { recursive: true });
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("SNOWLUMA_")));
  Object.assign(env, {
    SNOWLUMA_MCP_ENDPOINT: endpoint, SNOWLUMA_MCP_MODE: "write",
    SNOWLUMA_MCP_STREAM_DIR: directory, SNOWLUMA_MCP_UPLOAD_ROOT: temporary,
    SNOWLUMA_MCP_TIMEOUT_MS: "250", FLAT_SNOWLUMA_LOG: "off",
  });
  const child = spawn(command, args, { env, stdio: ["pipe", "pipe", "ignore"] });
  const closed = new Promise((done) => child.once("close", done));
  const pending = new Map();
  let id = 0;
  let failure;
  const fail = (error) => {
    failure = error;
    for (const request of pending.values()) { clearTimeout(request.timer); request.reject(error); }
    pending.clear();
  };
  child.on("error", fail);
  child.on("close", () => fail(new Error("MCP exited before replying")));
  child.stdin.on("error", fail);
  const lines = createInterface({ input: child.stdout });
  lines.on("line", (line) => {
    try {
      const response = JSON.parse(line);
      const request = pending.get(response.id);
      if (!request) return;
      pending.delete(response.id);
      clearTimeout(request.timer);
      if (response.error) request.reject(new Error(JSON.stringify(response.error)));
      else request.resolve(response.result);
    } catch (error) { fail(error); }
  });
  async function stop() {
    child.stdin.end();
    child.kill();
    const timer = setTimeout(() => child.kill("SIGKILL"), 2_000);
    try { await closed; } finally { clearTimeout(timer); lines.close(); }
  }
  function request(method, params) {
    return new Promise((resolve, reject) => {
      if (failure) { reject(failure); return; }
      const number = ++id;
      const timer = setTimeout(() => {
        pending.delete(number);
        reject(new Error(`${method} timeout`));
      }, 30_000);
      pending.set(number, { resolve, reject, timer });
      child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: number, method, params })}\n`);
    });
  }
  try {
    const info = await request("initialize", {
      protocolVersion: "2025-06-18", capabilities: {},
      clientInfo: { name: "execution-parity", version: "1" },
    });
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`);
    return { request, info, stop };
  } catch (error) { await stop(); throw error; }
}

async function normalize(result) {
  if (result.isError) return { isError: true };
  const value = result.structuredContent ?? JSON.parse(result.content.find((block) => block.type === "text").text);
  if (value.file_path && !value.chunk_count) {
    const bytes = await readFile(value.file_path);
    assert.equal(value.sha256, createHash("sha256").update(bytes).digest("hex"));
    value.file_bytes = bytes.toString("base64");
    delete value.file_path;
  }
  if (value.stream_id) assert.match(value.stream_id, /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i);
  delete value.stream_id;
  return value;
}

let official;
let flat;
try {
  official = await client(values["official-server"] ? "node" : "npx",
    values["official-server"] ? [resolve(values["official-server"])] : ["-y", `@snowluma/mcp@${catalog.source.version}`], join(temporary, "official"));
  assert.equal(official.info.serverInfo.version, catalog.source.version, "official MCP version must match the snapshot");
  flat = await client(resolve(values.binary), [], join(temporary, "flat"));
  const list = await flat.request("tools/list", {});
  assert.equal(list.tools.length, catalog.actions.length);
  assert.deepEqual(list.tools.map((tool) => tool.name).sort(), catalog.actions.map((action) => action.name).sort());
  const resource = await flat.request("resources/read", { uri: "snowluma://onebot/actions" });
  assert.equal(JSON.parse(resource.contents[0].text).hash, catalog.hash, "binary catalog must match the snapshot");
  const input = join(temporary, "source.bin");
  await writeFile(input, Buffer.alloc(512 * 1024 + 13, 120));
  const empty = join(temporary, "empty.bin");
  await writeFile(empty, "");
  const cases = [
    ["ordinary", "get_login_info", {}], ["logical-failure", "get_login_info", {}], ["http-failure", "get_login_info", {}],
    ...["download_file_stream", "download_file_image_stream", "download_file_record_stream"].map((name) => ["file", name, { file: "example" }]),
    ["decimal-integers", "download_file_stream", { file: "example" }],
    ["unknown-size", "download_file_stream", { file: "example" }], ["generic", "test_download_stream", {}],
    ["upload", "upload_file_stream", { file_retention: 300000, execution: { input_file: input } }],
    ["empty", "upload_file_stream", { execution: { input_file: empty } }],
    ["raw", "upload_file_stream", { stream_id: "manual", chunk_data: "aGVsbG8=" }],
    ["reset", "upload_file_stream", { stream_id: "manual", reset: true }],
    ...["bad-index", "missing-total", "bad-terminal", "trailing", "missing-retcode"].map((name) => [name, "download_file_stream", { file: "example" }]),
    ["too-many", "test_download_stream", {}], ["too-much-text", "test_download_stream", {}],
    ["long-healthy", "test_download_stream", {}], ["idle", "download_file_stream", { file: "example" }],
  ];
  const failures = new Set(["bad-index", "missing-total", "bad-terminal", "trailing", "missing-retcode", "too-many", "too-much-text", "idle"]);
  for (const [name, action, args] of cases) {
    scenario = name;
    const { execution, ...params } = args;
    const expected = await normalize(await official.request("tools/call", {
      name: "invoke_action", arguments: { action, params, ...(execution ? { execution } : {}) },
    }));
    const actual = await normalize(await flat.request("tools/call", { name: action, arguments: args }));
    assert.deepEqual(fixtureErrors, [], `${name}: mock server failure`);
    assert.equal(expected.isError ?? false, failures.has(name), `${name}: official fixture outcome differs`);
    assert.deepEqual(actual, expected, name);
  }
  console.log(`PASS: ${cases.length} scenarios match official @snowluma/mcp ${catalog.source.version}; results, file bytes, upload integrity and errors checked (${requests.length} HTTP requests).`);
} finally {
  await Promise.allSettled([flat?.stop(), official?.stop()]);
  server.closeAllConnections();
  await new Promise((done) => server.close(done));
  await rm(temporary, { recursive: true, force: true });
}
