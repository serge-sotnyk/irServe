#!/usr/bin/env node
// tools/probe/run.mjs
//
// Ad-hoc probe runner for the pinned `vercel/serve` reference. Spawns a
// throwaway serve instance against a fixture directory, executes a list of
// HTTP requests, and writes a deterministic Markdown + JSON report to
// tools/probe/results/<id>.{md,json}.
//
// Usage:
//   node tools/probe/run.mjs <probe-id>
//   node tools/probe/run.mjs --all
//   node tools/probe/run.mjs --list
//
// Case file schema lives in tools/probe/cases/_schema.json. This runner is
// intentionally dependency-free: it relies only on Node 18+ built-ins.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import net from 'node:net';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, '..', '..');
const CASES_DIR = join(__dirname, 'cases');
const RESULTS_DIR = join(__dirname, 'results');
const TMP_DIR = join(__dirname, '.tmp');
const SERVE_ENTRY = join(REPO_ROOT, 'third_party', 'serve', 'build', 'main.js');

const TRACKED_RESPONSE_HEADERS = [
  'content-type',
  'content-length',
  'cache-control',
  'etag',
  'last-modified',
  'location',
  'vary',
  'x-content-type-options',
];

function logInfo(msg) {
  process.stderr.write(`[probe] ${msg}\n`);
}

function logError(msg) {
  process.stderr.write(`[probe] ERROR: ${msg}\n`);
}

async function getFreePort() {
  return new Promise((resolveFn, rejectFn) => {
    const srv = net.createServer();
    srv.unref();
    srv.on('error', rejectFn);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => resolveFn(port));
    });
  });
}

async function readCase(probeId) {
  const path = join(CASES_DIR, `${probeId}.json`);
  const raw = await readFile(path, 'utf8');
  const parsed = JSON.parse(raw);
  parsed.__path = path;
  return parsed;
}

async function listCases() {
  const entries = await readdir(CASES_DIR);
  return entries
    .filter((e) => e.endsWith('.json') && e !== '_schema.json')
    .map((e) => e.replace(/\.json$/, ''))
    .sort();
}

async function materializeFixture(fixture, baseDir) {
  const files = fixture?.files ?? {};
  for (const [relPath, content] of Object.entries(files)) {
    const dest = join(baseDir, relPath);
    await mkdir(dirname(dest), { recursive: true });
    if (typeof content === 'string') {
      await writeFile(dest, content);
    } else if (content && typeof content === 'object' && 'base64' in content) {
      await writeFile(dest, Buffer.from(content.base64, 'base64'));
    } else {
      throw new Error(`Unsupported fixture entry for ${relPath}`);
    }
  }
  if (fixture?.serveJson) {
    const dest = join(baseDir, 'serve.json');
    await writeFile(dest, JSON.stringify(fixture.serveJson, null, 2));
  }
}

function spawnServe({ port, fixtureDir, extraArgs }) {
  const args = [
    SERVE_ENTRY,
    '--listen',
    String(port),
    '--no-clipboard',
    '--no-port-switching',
    ...(extraArgs ?? []),
    fixtureDir,
  ];
  const child = spawn(process.execPath, args, {
    cwd: REPO_ROOT,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, NO_UPDATE_NOTIFIER: '1', FORCE_COLOR: '0' },
  });
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', (d) => { stdout += d.toString(); });
  child.stderr.on('data', (d) => { stderr += d.toString(); });
  return { child, getStdout: () => stdout, getStderr: () => stderr };
}

async function waitReady(port, timeoutMs = 10000) {
  const deadline = Date.now() + timeoutMs;
  const url = `http://127.0.0.1:${port}/`;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(url, { redirect: 'manual' });
      // Any HTTP response means the server is up — even a 404 or 301.
      await r.arrayBuffer();
      return;
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  throw new Error(`serve did not become ready on port ${port} within ${timeoutMs}ms`);
}

async function killServe(child) {
  if (child.exitCode !== null) return;
  try {
    child.kill('SIGTERM');
  } catch { /* ignore */ }
  await new Promise((r) => setTimeout(r, 200));
  if (child.exitCode === null) {
    try { child.kill('SIGKILL'); } catch { /* ignore */ }
  }
}

function summarizeBody(buffer, contentType = '') {
  const sha256 = createHash('sha256').update(buffer).digest('hex');
  const length = buffer.length;
  const isTextual = /^(text\/|application\/(json|xml|javascript)|.*\+(json|xml))/i.test(contentType);
  let preview = null;
  if (isTextual) {
    const slice = buffer.slice(0, 200);
    preview = slice.toString('utf8');
  }
  return { length, sha256, preview };
}

function pickHeaders(headers) {
  const out = {};
  for (const name of TRACKED_RESPONSE_HEADERS) {
    const v = headers.get(name);
    if (v !== null) out[name] = v;
  }
  return out;
}

async function runRequest(port, req) {
  const url = `http://127.0.0.1:${port}${req.path}`;
  const init = {
    method: req.method ?? 'GET',
    headers: req.headers ?? {},
    redirect: req.redirect ?? 'manual',
  };
  if (req.body !== undefined) {
    init.body = req.body;
  }
  const res = await fetch(url, init);
  const buf = Buffer.from(await res.arrayBuffer());
  const headers = pickHeaders(res.headers);
  const body = summarizeBody(buf, headers['content-type']);
  return {
    name: req.name,
    method: init.method,
    path: req.path,
    requestHeaders: req.headers ?? {},
    status: res.status,
    statusText: res.statusText,
    headers,
    body,
  };
}

function renderMarkdown(probe, runMeta, results) {
  const lines = [];
  lines.push(`# Probe \`${probe.id}\``);
  lines.push('');
  if (probe.description) {
    lines.push(probe.description);
    lines.push('');
  }
  lines.push('## Run metadata');
  lines.push('');
  lines.push(`- Reference: \`serve@${runMeta.serveVersion ?? 'unknown'}\``);
  lines.push(`- Generated: \`${runMeta.generatedAt}\``);
  lines.push(`- Port: \`${runMeta.port}\``);
  if (probe.serveArgs?.length) {
    lines.push(`- Extra serve args: \`${probe.serveArgs.join(' ')}\``);
  }
  if (probe.fixture?.serveJson) {
    lines.push('- `serve.json` present in fixture.');
  }
  lines.push('');
  lines.push('## Requests');
  lines.push('');
  for (const r of results) {
    lines.push(`### \`${r.name}\``);
    lines.push('');
    lines.push(`- Request: \`${r.method} ${r.path}\``);
    if (Object.keys(r.requestHeaders).length) {
      lines.push(`- Request headers:`);
      for (const [k, v] of Object.entries(r.requestHeaders)) {
        lines.push(`  - \`${k}: ${v}\``);
      }
    }
    lines.push(`- Status: \`${r.status} ${r.statusText}\``);
    if (Object.keys(r.headers).length) {
      lines.push(`- Response headers:`);
      for (const [k, v] of Object.entries(r.headers)) {
        lines.push(`  - \`${k}: ${v}\``);
      }
    }
    lines.push(`- Body length: \`${r.body.length}\``);
    lines.push(`- Body sha256: \`${r.body.sha256}\``);
    if (r.body.preview !== null) {
      lines.push('- Body preview (≤200 bytes, utf-8):');
      lines.push('  ```');
      const preview = r.body.preview.replace(/\r/g, '');
      for (const ln of preview.split('\n')) lines.push(`  ${ln}`);
      lines.push('  ```');
    }
    lines.push('');
  }
  return lines.join('\n');
}

async function readServeVersion() {
  try {
    const pkg = JSON.parse(await readFile(join(REPO_ROOT, 'third_party', 'serve', 'package.json'), 'utf8'));
    return pkg.version;
  } catch {
    return null;
  }
}

async function runProbe(probeId) {
  const probe = await readCase(probeId);
  if (!probe.id) probe.id = probeId;
  await mkdir(RESULTS_DIR, { recursive: true });
  await mkdir(TMP_DIR, { recursive: true });
  const fixtureDir = await mkdtemp(join(TMP_DIR, `${probeId}-`));
  let port;
  let serveProc = null;
  let serveBundle = null;
  const runMeta = {
    generatedAt: new Date().toISOString(),
    serveVersion: await readServeVersion(),
  };
  try {
    await materializeFixture(probe.fixture ?? {}, fixtureDir);
    port = await getFreePort();
    runMeta.port = port;
    logInfo(`probe '${probeId}': serve@${runMeta.serveVersion} on port ${port}, fixture=${relative(REPO_ROOT, fixtureDir)}`);
    serveBundle = spawnServe({ port, fixtureDir, extraArgs: probe.serveArgs ?? [] });
    serveProc = serveBundle.child;
    await waitReady(port);
    const results = [];
    for (const req of probe.requests ?? []) {
      results.push(await runRequest(port, req));
    }
    const md = renderMarkdown(probe, runMeta, results);
    const json = {
      id: probeId,
      description: probe.description ?? null,
      runMeta,
      probeArgs: probe.serveArgs ?? [],
      results,
    };
    await writeFile(join(RESULTS_DIR, `${probeId}.md`), md);
    await writeFile(join(RESULTS_DIR, `${probeId}.json`), JSON.stringify(json, null, 2));
    logInfo(`probe '${probeId}': wrote results/${probeId}.md and .json (${results.length} requests)`);
    return { probeId, requests: results.length };
  } finally {
    if (serveProc) await killServe(serveProc);
    if (serveBundle && process.env.PROBE_VERBOSE) {
      const stderr = serveBundle.getStderr();
      if (stderr.trim()) logInfo(`probe '${probeId}' serve stderr:\n${stderr}`);
    }
    await rm(fixtureDir, { recursive: true, force: true });
  }
}

async function main() {
  const args = process.argv.slice(2);
  if (args.length === 0 || args.includes('--help') || args.includes('-h')) {
    process.stdout.write([
      'Usage:',
      '  node tools/probe/run.mjs <probe-id>',
      '  node tools/probe/run.mjs --all',
      '  node tools/probe/run.mjs --list',
      '',
      'Case files live in tools/probe/cases/<probe-id>.json.',
      'Results land in tools/probe/results/<probe-id>.{md,json}.',
      'Set PROBE_VERBOSE=1 to also print serve stderr after each run.',
      '',
    ].join('\n'));
    process.exit(args.length === 0 ? 1 : 0);
  }
  if (args[0] === '--list') {
    const ids = await listCases();
    for (const id of ids) process.stdout.write(`${id}\n`);
    return;
  }
  let ids;
  if (args[0] === '--all') {
    ids = await listCases();
  } else {
    ids = args;
  }
  let failed = 0;
  for (const id of ids) {
    try {
      await runProbe(id);
    } catch (e) {
      failed++;
      logError(`probe '${id}': ${e.message}`);
      if (process.env.PROBE_VERBOSE) logError(e.stack ?? '');
    }
  }
  if (failed > 0) {
    logError(`${failed} of ${ids.length} probes failed`);
    process.exit(1);
  }
}

main().catch((e) => {
  logError(e.stack ?? String(e));
  process.exit(2);
});
