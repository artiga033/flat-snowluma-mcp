#!/usr/bin/env node
// Audit the compiled MCP server against its pinned official documentation.
// Uses only Node.js built-ins; never calls a tool or connects to OneBot.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const RESOURCE = "snowluma://onebot/actions";
const AUTHORED = new Set([
  "cancel_group_album_media_like", "do_group_album_comment",
  "set_group_album_media_like", "upload_image_to_qun_album",
]);
const pointerKey = (key) => key.replaceAll("~", "~0").replaceAll("/", "~1");

/** Initialize and read declarations/resources with timeouts and child cleanup. */
export async function discover(binary) {
  const temporary = await mkdtemp(join(tmpdir(), "flat-snowluma-audit-"));
  let child;
  let finished;
  let lines;
  const pending = new Map();
  let closed;
  const fail = (error) => {
    closed ??= error;
    for (const { reject, timer } of pending.values()) {
      clearTimeout(timer);
      reject(error);
    }
    pending.clear();
  };
  try {
    const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("SNOWLUMA_")));
    env.SNOWLUMA_MCP_STREAM_DIR = join(temporary, "downloads");
    env.FLAT_SNOWLUMA_LOG = "off";
    child = spawn(resolve(binary), [], { env, stdio: ["pipe", "pipe", "pipe"] });
    finished = new Promise((done) => child.once("close", done));
    child.on("error", fail);
    child.on("close", (code, signal) => fail(new Error(`MCP exited (${signal ?? code})`)));
    child.stdin.on("error", fail);
    // Discard diagnostics: audit output contains declarations, not process logs.
    child.stderr.resume();
    lines = createInterface({ input: child.stdout });
    lines.on("error", fail);
    lines.on("line", (line) => {
      try {
        const message = JSON.parse(line);
        const request = pending.get(message.id);
        if (!request) return;
        pending.delete(message.id);
        clearTimeout(request.timer);
        if (message.error) request.reject(new Error(JSON.stringify(message.error)));
        else request.resolve(message.result);
      } catch (error) {
        fail(new Error(`Invalid MCP response: ${error.message}`));
      }
    });
    let id = 0;
    const send = (message) => child.stdin.write(`${JSON.stringify(message)}\n`);
    const request = (method, params) => new Promise((resolve, reject) => {
      if (closed) { reject(closed); return; }
      const requestId = ++id;
      const timer = setTimeout(() => fail(new Error(`MCP ${method} timed out`)), 30_000);
      pending.set(requestId, { resolve, reject, timer });
      send({ jsonrpc: "2.0", id: requestId, method, params });
    });
    const info = await request("initialize", {
      protocolVersion: "2025-06-18", capabilities: {},
      clientInfo: { name: "flat-snowluma-audit", version: "1" },
    });
    send({ jsonrpc: "2.0", method: "notifications/initialized" });
    const tools = [];
    const cursors = new Set();
    let cursor;
    do {
      const result = await request("tools/list", cursor ? { cursor } : {});
      assert.ok(Array.isArray(result.tools), "tools/list must return tools");
      tools.push(...result.tools);
      cursor = result.nextCursor;
      if (cursor) {
        assert.ok(!cursors.has(cursor), "tools/list repeated its pagination cursor");
        cursors.add(cursor);
      }
    } while (cursor);
    const resource = await request("resources/read", { uri: RESOURCE });
    const content = resource.contents.find((item) => item.uri === RESOURCE && typeof item.text === "string");
    assert.ok(content, "catalog resource has no JSON text");
    return { info, tools, catalog: JSON.parse(content.text) };
  } finally {
    lines?.close();
    fail(new Error("Audit connection closed"));
    if (child) {
      child.stdin.end();
      child.kill();
      const timer = setTimeout(() => child.kill("SIGKILL"), 2_000);
      try { await finished; } finally { clearTimeout(timer); }
    }
    await rm(temporary, { recursive: true, force: true });
  }
}

function officialDescription(action, prefix) {
  if (action.summary) return { text: action.summary, source: `${prefix}/summary` };
  if (["can_send_image", "can_send_record"].includes(action.name)) {
    return { text: action.returnsSchema.properties.yes.description,
      source: `${prefix}/returnsSchema/properties/yes/description` };
  }
  if (action.name === "get_group_album_list") {
    return { text: action.returnsSchema.description, source: `${prefix}/returnsSchema/description` };
  }
  if (action.returns) return { text: action.returns, source: `${prefix}/returns` };
  assert.ok(AUTHORED.has(action.name), `${action.name}: no official description or known authored fallback`);
  return { source: "authored:src/patches.rs" };
}

// Rust serializes absent optional documentation fields as null/defaults.
// Compare every original field, allowing those serialization-only additions.
function documentedFields(value, reference) {
  if (Array.isArray(reference)) {
    assert.ok(Array.isArray(value) && value.length === reference.length, "catalog resource array differs");
    return reference.map((item, index) => documentedFields(value[index], item));
  }
  if (reference !== null && typeof reference === "object") {
    assert.ok(value !== null && typeof value === "object", "catalog resource object is missing");
    return Object.fromEntries(Object.keys(reference).map((key) => [key, documentedFields(value[key], reference[key])]));
  }
  return value;
}

/** Validate the flattened action tools and the official MCP execution option. */
export function audit(catalog, actual) {
  const digest = createHash("sha256").update(JSON.stringify({ categories: catalog.categories, actions: catalog.actions, execution: catalog.execution })).digest("hex");
  assert.equal(digest, catalog.hash, "pinned catalog hash differs from its content");
  assert.equal(actual.catalog.version, catalog.source.version, "binary has a stale catalog version; rebuild it");
  assert.equal(actual.catalog.hash, catalog.hash, "binary has a different catalog hash; rebuild it");
  assert.deepEqual(documentedFields(actual.catalog.actions, catalog.actions), catalog.actions, "binary resource actions differ from the snapshot");
  assert.deepEqual(actual.catalog.categories, catalog.categories, "binary resource categories differ from the snapshot");
  assert.deepEqual(actual.catalog.execution, catalog.execution, "binary execution schema differs from the snapshot");
  const actions = new Map(catalog.actions.map((action, index) => [action.name, { action, index }]));
  assert.equal(actions.size, catalog.actions.length, "duplicate official action name");
  assert.equal(new Set(actual.tools.map((tool) => tool.name)).size, actual.tools.length, "duplicate MCP tool name");
  assert.deepEqual(actual.tools.map((tool) => tool.name).sort(), [...actions.keys()].sort(), "MCP tools must match official action names exactly");
  const entries = [];
  let officialParameters = 0;
  for (const tool of [...actual.tools].sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)) {
    const name = tool.name;
    const { action, index } = actions.get(name);
    const prefix = `assets/catalog.json#/actions/${index}`;
    const expected = structuredClone(action.inputSchema);
    if (name === "upload_file_stream") {
      expected.properties.execution = structuredClone(catalog.execution.inputSchema.properties.execution);
      const required = (expected.required ?? []).filter((field) => field !== "stream_id");
      if (required.length) expected.required = required; else delete expected.required;
      // Repeat the official fields in each branch for clients that render unions
      // independently. Descriptions and field schemas still match the snapshot.
      const manual = structuredClone(expected);
      manual.required = ["stream_id"];
      const automatic = structuredClone(expected);
      automatic.required = ["execution"];
      automatic.properties.execution.required = ["input_file"];
      expected.anyOf = [manual, automatic];
    }
    assert.deepEqual(tool.inputSchema, expected, `${name}: action/execution inputSchema differs`);
    assert.equal(tool.annotations?.readOnlyHint, action.readOnly, `${name}: readOnly differs`);
    if (!action.readOnly) assert.equal(tool.annotations.destructiveHint, true, `${name}: missing destructive hint`);
    const base = officialDescription(action, prefix);
    if (base.text === undefined) base.text = tool.description?.split("\n")[0];
    assert.ok(typeof base.text === "string" && base.text.trim(), `${name}: empty description`);
    const segments = [base, ...action.invariants.map((text, i) => ({ text, source: `${prefix}/invariants/${i}` }))];
    if (action.aliases.length) segments.push({ text: action.aliases.join(" "), source: `${prefix}/aliases` });
    assert.equal(tool.description, segments.map((part) => part.text).join("\n"), `${name}: description differs from official wording`);
    const properties = tool.inputSchema.properties;
    for (const param of action.params) {
      for (const [key, schemaKey] of [["desc", "description"], ["default", "default"], ["values", "enum"], ["role", "x-role"]]) {
        if (Object.hasOwn(param, key)) assert.deepEqual(properties[param.name][schemaKey], param[key], `${name}.${param.name}: ${schemaKey} differs`);
      }
    }
    officialParameters += action.params.length;
    const entry = { name, kind: "official-action", nameSource: `${prefix}/name`, declaration: tool,
      descriptionSegments: segments, aliases: action.aliases, inputSchemaSource: `${prefix}/inputSchema`, parameters: [] };
    if (name === "upload_file_stream") {
      entry.executionSource = "assets/catalog.json#/execution/inputSchema/properties/execution";
      entry.inputSchemaAdaptation = { source: "authored:src/tools.rs", reason: "stream_id is generated when execution.input_file is provided; required for manual chunks" };
    }
    if (action.returnsSchema) {
      assert.deepEqual(tool.outputSchema, { type: "object", if: { properties: { status: { const: "ok" } }, required: ["status"] }, then: { properties: { data: action.returnsSchema } } }, `${name}: success data schema differs`);
      entry.outputSchemaSource = { data: `${prefix}/returnsSchema`, envelope: "authored:src/tools.rs (validate data only for successful OneBot envelopes)" };
    } else assert.equal(tool.outputSchema, undefined, `${name}: unexpected outputSchema`);
    for (const [param, schema] of Object.entries(properties)) {
      const source = param === "execution" ? entry.executionSource : `${prefix}/inputSchema/properties/${pointerKey(param)}`;
      const item = { name: param, source, schema };
      if (param === "execution") item.fields = [{ name: "input_file", source: `${source}/properties/input_file`, schema: schema.properties.input_file }];
      entry.parameters.push(item);
    }
    entries.push(entry);
  }
  return {
    source: catalog.source, fetchedAt: catalog.fetchedAt, catalogHash: catalog.hash,
    checks: { officialNames: actions.size, exactOfficialInputSchemas: actions.size - 1,
      officialParameters, descriptionComposition: actions.size, officialExecutionOptions: 1, flattenedUploadSchemas: 1 },
    authoredDescriptions: entries.filter((entry) => entry.descriptionSegments[0].source.startsWith("authored:")).map((entry) => entry.name),
    serverInstructions: { text: actual.info.instructions, source: "authored:src/server.rs" },
    tools: entries,
  };
}

const cell = (value) => String(value).replaceAll("|", "\\|").replaceAll("\n", "<br>");

export function markdown(report) {
  const lines = ["# 工具来源审计", "",
    `官方文档：\`${report.source.package}\` ${report.source.version}；抓取时间 \`${report.fetchedAt}\`。`,
    `目录 hash：\`${report.catalogHash}\`。`, "",
    `检查通过：${report.checks.officialNames} 个官方工具、${report.checks.officialParameters} 个官方参数，含 1 个官方 MCP 本地主机执行选项。`, "",
    "## 来源规则", "",
    "- 所有官方工具名称及 action 参数字段 schema 与快照完全一致；描述中的官方摘要、约束和别名保留原文。",
    "- 缺失摘要时复用已有官方返回描述；仍缺失时将补充文本标注为自写。自写文本仅记录来源，不据此判断其业务语义。",
    "- 官方返回 schema 原样约束成功响应的 data 字段；外层条件封套由本项目定义，失败响应保留完整 OneBot 数据。",
    "- upload_file_stream 增加官方 invoke_action 的 execution 选项；stream_id 在自动上传时由 MCP 生成，原始字段仍全部保留，必填条件调整为手动分块或自动上传二选一。",
    "- JSON 报告包含完整工具声明、参数 schema、描述原文及来源 JSON Pointer。", "",
    "## 自写描述", "", "| 工具 | 描述 | 来源 |", "| --- | --- | --- |"];
  for (const entry of report.tools) {
    if (report.authoredDescriptions.includes(entry.name)) {
      lines.push(`| \`${entry.name}\` | ${cell(entry.declaration.description)} | ${entry.descriptionSegments[0].source} |`);
    }
  }
  lines.push("", "## MCP 执行选项", "", "`upload_file_stream.execution` 和 `execution.input_file` 原文来自官方 MCP。", "",
    "手动分块需要 stream_id；自动上传需要 execution.input_file。此条件适配由本项目定义，不改变任何官方 action 参数字段。", "");
  lines.push("", "## 服务器说明", "", "来源：`authored:src/server.rs`。", "", report.serverInstructions.text ?? "", "",
    "## 逐工具清单", "", "参数 ? 表示非无条件必填；upload_file_stream 的 stream_id/execution 存在上述条件要求。完整描述与逐参数来源见 [tool-audit.json](tool-audit.json)。", "",
    "| 工具 | 参数 | 描述来源 |", "| --- | --- | --- |");
  for (const entry of report.tools) {
    const required = entry.declaration.inputSchema.required ?? [];
    const params = entry.parameters.map((param) => `\`${param.name}${required.includes(param.name) ? "" : "?"}\``).join(", ") || "—";
    const source = entry.descriptionSegments.map((segment) => segment.source).join("<br>");
    lines.push(`| \`${entry.name}\` | ${params} | ${cell(source)} |`);
  }
  return `${lines.join("\n")}\n`;
}

async function main() {
  const { values } = parseArgs({ options: {
    binary: { type: "string" }, "output-dir": { type: "string", default: join(ROOT, "docs") },
    help: { type: "boolean", short: "h" },
  } });
  if (values.help) {
    console.log("Usage: node scripts/audit-tools.mjs --binary <MCP binary> [--output-dir <directory>]");
    return;
  }
  assert.ok(values.binary, "--binary is required");
  const catalog = JSON.parse(await readFile(join(ROOT, "assets", "catalog.json"), "utf8"));
  const report = audit(catalog, await discover(values.binary));
  await mkdir(values["output-dir"], { recursive: true });
  await writeFile(join(values["output-dir"], "tool-audit.json"), `${JSON.stringify(report, null, 2)}\n`);
  await writeFile(join(values["output-dir"], "tool-audit.md"), markdown(report));
  console.log(JSON.stringify({ source: report.source, checks: report.checks, authoredDescriptions: report.authoredDescriptions }, null, 2));
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  main().catch((error) => {
    console.error(`Audit failed: ${error.message.split("\n")[0]}`);
    process.exitCode = 1;
  });
}
