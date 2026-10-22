// Live end-to-end check against the configured SnowLuma instance.
//
// Manually probes tool discovery and live OneBot operations:
//   1. MCP tools/list discovery (names and descriptions)
//   2. get_group_msg_history and mapping the result
//   3. send_group_msg with a segment array (a real send)
//   4. upload_file_stream with execution.input_file -> send_group_msg
//   5. download a recent image, if available
// Requires SNOWLUMA_MCP_ENDPOINT and SNOWLUMA_TEST_GROUP; set
// SNOWLUMA_MCP_TOKEN if the instance requires authentication.
// This script sends real messages and uploads files to the configured group.
import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const binary = process.argv[2];
if (!binary) throw new Error("usage: node scripts/live-check.mjs <binary>");
const endpoint = process.env.SNOWLUMA_MCP_ENDPOINT?.trim();
if (!endpoint) throw new Error("SNOWLUMA_MCP_ENDPOINT is required");
const endpointUrl = new URL(endpoint);
if (!["http:", "https:"].includes(endpointUrl.protocol)) {
  throw new Error("SNOWLUMA_MCP_ENDPOINT must use http or https");
}
const rawGroupId = process.env.SNOWLUMA_TEST_GROUP?.trim();
if (!rawGroupId || !/^[1-9]\d*$/.test(rawGroupId)) {
  throw new Error("SNOWLUMA_TEST_GROUP must be a positive integer");
}
const groupId = Number(rawGroupId);
if (!Number.isSafeInteger(groupId)) {
  throw new Error("SNOWLUMA_TEST_GROUP must be a safe integer");
}

const share = mkdtempSync(join(tmpdir(), "fsmcp-live-"));
const uploadPath = join(share, "report.txt");
writeFileSync(uploadPath, `flat-snowluma-mcp live check ${new Date().toISOString()}\n`);

const env = {
  ...process.env,
  SNOWLUMA_MCP_ENDPOINT: endpoint,
  SNOWLUMA_MCP_UPLOAD_ROOT: share,
  SNOWLUMA_MCP_STREAM_DIR: join(share, "downloads"),
};
const child = spawn(binary, [], { env, stdio: ["pipe", "pipe", "pipe"] });
let buf = "";
const pending = new Map();
let id = 1;
child.stdout.setEncoding("utf8");
child.stdout.on("data", (c) => {
  buf += c;
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    if (!line) continue;
    const m = JSON.parse(line);
    if (m.id && pending.has(m.id)) {
      pending.get(m.id)(m);
      pending.delete(m.id);
    }
  }
});
child.stderr.setEncoding("utf8");
child.stderr.on("data", (c) => process.stderr.write(`[stderr] ${c}`));
const req = (method, params) =>
  new Promise((resolve) => {
    const n = id++;
    pending.set(n, resolve);
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: n, method, params }) + "\n");
  });
const notify = (method, params) =>
  child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method, params }) + "\n");

const call = (name, args) => req("tools/call", { name, arguments: args });

await req("initialize", {
  protocolVersion: "2025-06-18",
  capabilities: {},
  clientInfo: { name: "live-check", version: "1" },
});
notify("notifications/initialized", {});

const tools = (await req("tools/list", {})).result.tools;
console.log(`1. discovery: ${tools.length} tools; sample names include`,
  tools.slice(0, 3).map((t) => t.name).join(", "));

// 2. Read recent group history.
const history = await call("get_group_msg_history", { group_id: groupId, count: 3 });
if (history.result.isError) {
  console.log("2. history ERROR:", history.result.content[0].text);
} else {
  const messages = history.result.structuredContent.data.messages.map((m) => ({
    id: m.message_id,
    text: (m.raw_message ?? "").slice(0, 40),
  }));
  console.log("2. history ok:", JSON.stringify(messages));
}

// 3. send a text message.
const sent = await call("send_group_msg", {
  group_id: groupId,
  message: [{ type: "text", data: { text: "flat-snowluma-mcp live check" } }],
});
console.log(
  "3. send_group_msg:",
  sent.result.isError ? `ERROR ${sent.result.content[0].text}` : JSON.stringify(sent.result.structuredContent.data),
);

// 4. upload then send the uploaded path.
const upload = await call("upload_file_stream", { execution: { input_file: uploadPath } });
if (upload.result.isError) {
  console.log("4. upload ERROR:", upload.result.content[0].text);
} else {
  const info = upload.result.structuredContent;
  console.log("4. upload ok:", JSON.stringify(info));
  const sentFile = await call("send_group_msg", {
    group_id: groupId,
    message: [{ type: "file", data: { file: info.file_path, name: "report.txt" } }],
  });
  console.log(
    "4. send uploaded file:",
    sentFile.result.isError
      ? `ERROR ${sentFile.result.content[0].text}`
      : JSON.stringify(sentFile.result.structuredContent.data),
  );
}

// 5. download a cached image by its file name and confirm the bytes land locally.
const recentImage = (
  await call("get_group_msg_history", { group_id: groupId, count: 40 })
).result.structuredContent?.data?.messages
  ?.flatMap((m) => m.message ?? [])
  .find((seg) => seg.type === "image" && seg.data?.file)?.data?.file;

const download = await call("download_file_image_stream", {
  file: recentImage ?? "none.jpg",
});
console.log(
  "5. download_file_image_stream:",
  download.result.isError
    ? `ERROR ${download.result.content[0].text}`
    : JSON.stringify(download.result.structuredContent),
);

child.kill();
process.exit(0);
