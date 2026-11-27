#!/usr/bin/env node
// Moves a personal instance's D1 rows from one database to another (docs/MIGRATION.md, R5.1).
//
//   node tools/data-move.mjs export <source-d1-uuid> <out-dir>
//   node tools/data-move.mjs import <target-d1-uuid> <in-dir>
//   node tools/data-move.mjs verify <target-d1-uuid> <in-dir>
//
// Export only reads (SELECT). Import inserts and never deletes or updates; it refuses a target that already has rows.
// Everything goes through the `cf` CLI, which must already be logged in. Rows are written as JSON, one file per table.

import { spawnSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const SKIP_TABLES = new Set(["d1_migrations", "_cf_KV"]);
const MAX_BATCH_CHARS = 20000;

// The cf CLI is a Node script. Calling it through the Node binary passes each argument intact (no shell quoting).
const CF_BIN =
  process.env.CF_BIN ??
  "C:/Users/idant/AppData/Local/vite-plus/data/js_runtime/node/26.13.0/node_modules/cf/bin/cf";

function cf(args) {
  const run = spawnSync(process.execPath, [CF_BIN, ...args], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (run.status !== 0) {
    throw new Error(`cf ${args.slice(0, 2).join(" ")} failed (exit ${run.status}):\n${run.stderr || run.stdout || run.error?.message || "no output"}`);
  }
  return run.stdout;
}

/** Runs one SQL statement and returns its result rows. The CLI prints an array of result sets. */
function query(db, sql) {
  const out = JSON.parse(cf(["d1", "query", db, "--sql", sql]));
  const first = Array.isArray(out) ? out[0] : out;
  if (!first?.success) throw new Error(`query failed: ${sql.slice(0, 120)}`);
  return first.results ?? [];
}

function tablesOf(db) {
  return query(db, "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name;")
    .map((r) => r.name)
    .filter((name) => !SKIP_TABLES.has(name));
}

const quote = (ident) => `"${ident.replace(/"/g, '""')}"`;

function literal(value) {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "number") return Number.isFinite(value) ? String(value) : "NULL";
  if (typeof value === "boolean") return value ? "1" : "0";
  return `'${String(value).replace(/'/g, "''")}'`;
}

function exportAll(source, outDir) {
  mkdirSync(outDir, { recursive: true });
  const counts = {};
  for (const table of tablesOf(source)) {
    const rows = query(source, `SELECT * FROM ${quote(table)};`);
    writeFileSync(join(outDir, `${table}.json`), JSON.stringify(rows, null, 2));
    counts[table] = rows.length;
    console.log(`export ${table}: ${rows.length}`);
  }
  writeFileSync(
    join(outDir, "manifest.json"),
    JSON.stringify({ exportedAt: new Date().toISOString(), counts }, null, 2),
  );
}

function readManifest(inDir) {
  const path = join(inDir, "manifest.json");
  if (!existsSync(path)) throw new Error(`no manifest.json in ${inDir}; run export first`);
  return JSON.parse(readFileSync(path, "utf8"));
}

/** Parents before children, from each table's foreign keys, so a child row never lands before its parent. */
function importOrder(db, tables) {
  const parents = new Map();
  for (const table of tables) {
    const links = query(db, `PRAGMA foreign_key_list(${quote(table)});`).map((r) => r.table);
    parents.set(table, new Set(links.filter((p) => tables.includes(p) && p !== table)));
  }
  const order = [];
  const pending = new Map(parents);
  while (pending.size > 0) {
    const ready = [...pending].filter(([, deps]) => [...deps].every((d) => order.includes(d)));
    if (ready.length === 0) throw new Error("foreign-key cycle between: " + [...pending.keys()].join(", "));
    for (const [table] of ready) {
      order.push(table);
      pending.delete(table);
    }
  }
  return order;
}

function importAll(target, inDir) {
  const { counts } = readManifest(inDir);
  const existing = tablesOf(target);
  for (const table of Object.keys(counts)) {
    if (!existing.includes(table)) throw new Error(`target has no table ${table}; apply the migration first`);
    const [{ n }] = query(target, `SELECT COUNT(*) AS n FROM ${quote(table)};`);
    if (n !== 0) throw new Error(`target table ${table} already has ${n} rows; refusing to import into it`);
  }
  for (const table of importOrder(target, Object.keys(counts))) {
    const rows = JSON.parse(readFileSync(join(inDir, `${table}.json`), "utf8"));
    const statements = rows.map((row) => {
      const columns = Object.keys(row);
      const values = columns.map((c) => literal(row[c]));
      return `INSERT INTO ${quote(table)} (${columns.map(quote).join(", ")}) VALUES (${values.join(", ")});`;
    });
    // Windows limits a command line to about 32k characters, so batches are cut by size, not by row count.
    let batch = [];
    let size = 0;
    const flush = (index) => {
      if (batch.length === 0) return;
      try {
        query(target, batch.join("\n"));
      } catch (e) {
        throw new Error(`import ${table} failed near row ${index}: ${e.message}`);
      }
      batch = [];
      size = 0;
    };
    statements.forEach((statement, index) => {
      if (statement.length > MAX_BATCH_CHARS) {
        throw new Error(`import ${table} row ${index} is ${statement.length} characters; too large for one command`);
      }
      if (size + statement.length > MAX_BATCH_CHARS) flush(index);
      batch.push(statement);
      size += statement.length;
    });
    flush(statements.length);
    console.log(`import ${table}: ${rows.length}`);
  }
}

function verify(target, inDir) {
  const { counts } = readManifest(inDir);
  let ok = true;
  for (const [table, expected] of Object.entries(counts)) {
    const [{ n }] = query(target, `SELECT COUNT(*) AS n FROM ${quote(table)};`);
    const same = n === expected;
    ok &&= same;
    console.log(`${same ? "ok  " : "DIFF"} ${table}: target ${n}, exported ${expected}`);
  }
  if (!ok) throw new Error("row counts differ");
  console.log("all tables match");
}

const [command, db, dir] = process.argv.slice(2);
if (!command || !db || !dir) {
  console.error("usage: data-move.mjs export|import|verify <d1-uuid> <dir>");
  process.exit(2);
}
if (command === "export") exportAll(db, dir);
else if (command === "import") importAll(db, dir);
else if (command === "verify") verify(db, dir);
else {
  console.error(`unknown command ${command}`);
  process.exit(2);
}
