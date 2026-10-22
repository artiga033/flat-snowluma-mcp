#!/usr/bin/env node
// Regenerate assets/catalog.json from the official SnowLuma docs MCP server.
//
// The action catalog is fetched from the official docs MCP
// explicitly and pinned here. We drive the MCP server over stdio exactly the
// way a client would (initialize → list_actions → get_action × N →
// list_categories), so the snapshot reflects the documented contract rather
// than an internal file layout.
//
// Usage:
//   node scripts/generate-catalog.mjs                 # pin = 2.1.1
//   SNOWLUMA_MCP_VERSION=2.1.1 node scripts/generate-catalog.mjs
//
// The output is committed. `src/catalog.rs` embeds it with include_str!.

import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VERSION = process.env.SNOWLUMA_MCP_VERSION?.trim() || "2.1.1";
const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..");
const outFile = join(repo, "assets", "catalog.json");

const PROTOCOL_VERSION = "2025-06-18";

/** Minimal newline-delimited JSON-RPC client over a stdio MCP server. */
function makeClient(command, args, env) {
  const child = spawn(command, args, {
    env: { ...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("SNOWLUMA_"))), ...env },
    stdio: ["pipe", "pipe", "pipe"],
  });

  let buffer = "";
  const pending = new Map();
  const stderr = [];
  let nextId = 1;

  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    buffer += chunk;
    let index;
    while ((index = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, index).trim();
      buffer = buffer.slice(index + 1);
      if (!line) continue;
      let message;
      try {
        message = JSON.parse(line);
      } catch {
        continue; // ignore non-protocol noise on stdout
      }
      if (message.id !== undefined && pending.has(message.id)) {
        const { resolve, reject } = pending.get(message.id);
        pending.delete(message.id);
        if (message.error) reject(new Error(JSON.stringify(message.error)));
        else resolve(message.result);
      }
    }
  });
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => stderr.push(chunk));

  const send = (message) => child.stdin.write(`${JSON.stringify(message)}\n`);

  const request = (method, params) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject });
      send({ jsonrpc: "2.0", id, method, params });
    });

  const notify = (method, params) => send({ jsonrpc: "2.0", method, params });

  const close = () => {
    child.stdin.end();
    child.kill("SIGTERM");
  };

  return { request, notify, close, stderr: () => stderr.join("") };
}

function parseToolJson(result, toolName) {
  const text = result?.content?.find((block) => block.type === "text")?.text;
  if (typeof text !== "string") {
    throw new Error(`${toolName}: tool result had no text content`);
  }
  return JSON.parse(text);
}

async function main() {
  const rawMetadata = JSON.parse(execFileSync(
    "npm", ["view", `@snowluma/mcp@${VERSION}`, "version", "gitHead", "--json"],
    { encoding: "utf8", timeout: 30_000 },
  ));
  const metadata = Array.isArray(rawMetadata) ? rawMetadata[0] : rawMetadata;
  if (!metadata?.version || !metadata?.gitHead) {
    throw new Error("official package metadata lacks version or gitHead");
  }
  // Resolve tags before launching MCP, so recorded and queried versions match.
  const version = metadata.version;
  const temporary = mkdtempSync(join(tmpdir(), "flat-snowluma-catalog-"));
  const client = makeClient(
    "npx",
    ["-y", `@snowluma/mcp@${version}`],
    {
      // Write mode exposes invoke_action's execution schema. No execution tool
      // is called and this endpoint is never contacted.
      SNOWLUMA_MCP_MODE: "write",
      SNOWLUMA_MCP_ENDPOINT: "http://127.0.0.1:1",
      SNOWLUMA_MCP_STREAM_DIR: temporary,
    },
  );

  try {
    await client.request("initialize", {
      protocolVersion: PROTOCOL_VERSION,
      capabilities: {},
      clientInfo: { name: "flat-snowluma-mcp-catalog", version: "0.1.0" },
    });
    client.notify("notifications/initialized", {});

    const listed = parseToolJson(
      await client.request("tools/call", { name: "list_actions", arguments: {} }),
      "list_actions",
    );
    if (!Array.isArray(listed.actions)) {
      throw new Error("list_actions did not return an actions array");
    }

    const actions = [];
    for (const entry of listed.actions) {
      const doc = parseToolJson(
        await client.request("tools/call", {
          name: "get_action",
          arguments: { name: entry.name },
        }),
        `get_action(${entry.name})`,
      );
      if (doc.name !== entry.name) {
        throw new Error(`get_action(${entry.name}) returned ${doc.name}`);
      }
      actions.push(doc);
    }

    const categories = parseToolJson(
      await client.request("tools/call", { name: "list_categories", arguments: {} }),
      "list_categories",
    );

    actions.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));

    const tools = await client.request("tools/list", {});
    const invoke = tools.tools.find((tool) => tool.name === "invoke_action");
    if (!invoke?.inputSchema?.properties?.execution) {
      throw new Error("official MCP is missing invoke_action's execution schema");
    }
    const execution = {
      sourceTool: invoke.name,
      description: invoke.description,
      inputSchema: invoke.inputSchema,
    };
    const canonical = JSON.stringify({ categories, actions, execution });
    const hash = createHash("sha256").update(canonical).digest("hex");

    const snapshot = {
      source: { package: "@snowluma/mcp", version, gitCommit: metadata.gitHead },
      fetchedAt: new Date().toISOString(),
      hash,
      categories,
      actions,
      execution,
    };

    mkdirSync(dirname(outFile), { recursive: true });
    writeFileSync(outFile, `${JSON.stringify(snapshot, null, 2)}\n`);

    const aliases = actions.reduce((sum, action) => sum + action.aliases.length, 0);
    const stream = actions.filter((action) => action.stream).length;
    console.log(
      `Wrote ${outFile}\n  version=${version} actions=${actions.length} ` +
        `aliases=${aliases} stream=${stream} categories=${categories.length}\n  hash=${hash}`,
    );
  } finally {
    client.close();
    rmSync(temporary, { recursive: true, force: true });
    const err = client.stderr().trim();
    if (err) console.error(`[server stderr]\n${err}`);
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
