#!/usr/bin/env node
// tools/probe/run.mjs
//
// Ad-hoc probe runner for the pinned `vercel/serve` reference. Spawns a
// throwaway serve instance against a fixture directory, executes a list of
// HTTP requests, and writes a deterministic Markdown + JSON report to
// tools/probe/results/<id>.{md,json}.
//
// Usage:
//   node tools/probe/run.mjs <probe-id> [--snapshot=update|verify|none]
//   node tools/probe/run.mjs --all       [--snapshot=update|verify|none]
//   node tools/probe/run.mjs --list
//
// Case file schema lives in tools/probe/cases/_schema.json. Snapshot schema
// lives in tools/probe/snapshots/_schema.json. This runner is intentionally
// dependency-free: it relies only on Node 18+ built-ins.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import net from 'node:net';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, '..', '..');
const CASES_DIR = join(__dirname, 'cases');
const RESULTS_DIR = join(__dirname, 'results');
const SNAPSHOTS_DIR = join(__dirname, 'snapshots');
const TMP_DIR = join(__dirname, '.tmp');
const SERVE_ENTRY = join(REPO_ROOT, 'third_party', 'serve', 'build', 'main.js');

const DEFAULT_VOLATILE_HEADERS = ['last-modified'];

const TRACKED_RESPONSE_HEADERS = [
  'accept-ranges',
  'cache-control',
  'content-length',
  'content-range',
  'content-type',
  'etag',
  'last-modified',
  'location',
  'vary',
  'x-content-type-options',
  'access-control-allow-origin',
  'access-control-allow-methods',
  'access-control-allow-headers',
  'access-control-allow-credentials',
  'access-control-expose-headers',
  'access-control-max-age',
  'access-control-allow-private-network',
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
  if (req.mode === 'raw') return runRequestRaw(port, req);
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
    mode: 'fetch',
    method: init.method,
    path: req.path,
    requestHeaders: req.headers ?? {},
    status: res.status,
    statusText: res.statusText,
    headers,
    body,
  };
}

// Raw-socket request mode. Sends the literal `path` bytes in the request line
// without any client-side normalization (no `..` collapsing, no `%2e%2e`
// decoding, no `//` collapsing). Used for wire-level probes such as
// path-traversal verification — `fetch` and `http.request` both pre-normalize
// paths, which masks the on-the-wire behavior of the reference implementation.
//
// Limitations: this minimal parser handles the response shape serve actually
// emits for small static-file responses (status line, header block, body
// terminated by `Content-Length` or by EOF after `Connection: close`).
// It does NOT decode `Transfer-Encoding: chunked`. To keep things simple,
// the runner always injects `Connection: close` so the server closes the
// socket after the response, giving us a deterministic EOF.
async function runRequestRaw(port, req) {
  const method = req.method ?? 'GET';
  const path = req.path;
  const userHeaders = { ...(req.headers ?? {}) };
  // Inject Host and Connection: close unless caller already provided them.
  const lowerKeys = Object.fromEntries(Object.keys(userHeaders).map((k) => [k.toLowerCase(), k]));
  if (!('host' in lowerKeys)) userHeaders.Host = `127.0.0.1:${port}`;
  if (!('connection' in lowerKeys)) userHeaders.Connection = 'close';
  const bodyText = req.body ?? '';
  if (bodyText && !('content-length' in lowerKeys)) {
    userHeaders['Content-Length'] = String(Buffer.byteLength(bodyText, 'utf8'));
  }
  const headerBlock = Object.entries(userHeaders).map(([k, v]) => `${k}: ${v}`).join('\r\n');
  const requestText = `${method} ${path} HTTP/1.1\r\n${headerBlock}\r\n\r\n${bodyText}`;
  const requestLineCapture = `${method} ${path} HTTP/1.1`;

  return new Promise((resolveFn, rejectFn) => {
    const socket = net.connect(port, '127.0.0.1');
    let raw = Buffer.alloc(0);
    let settled = false;
    const fail = (err) => {
      if (settled) return;
      settled = true;
      try { socket.destroy(); } catch { /* ignore */ }
      rejectFn(err);
    };
    socket.setTimeout(10000, () => fail(new Error('raw request timed out')));
    socket.on('error', fail);
    socket.once('connect', () => {
      socket.write(requestText, 'utf8');
    });
    socket.on('data', (chunk) => { raw = Buffer.concat([raw, chunk]); });
    socket.on('end', () => {
      if (settled) return;
      settled = true;
      try {
        const sep = raw.indexOf('\r\n\r\n');
        if (sep < 0) throw new Error('raw response missing header terminator');
        const headBytes = raw.slice(0, sep).toString('utf8');
        const bodyBuf = raw.slice(sep + 4);
        const lines = headBytes.split('\r\n');
        const statusLine = lines[0] ?? '';
        const m = /^HTTP\/\d\.\d\s+(\d+)\s+(.*)$/.exec(statusLine);
        if (!m) throw new Error(`raw response bad status line: ${JSON.stringify(statusLine)}`);
        const status = parseInt(m[1], 10);
        const statusText = m[2];
        const allHeaders = {};
        for (const line of lines.slice(1)) {
          const colon = line.indexOf(':');
          if (colon < 0) continue;
          const name = line.slice(0, colon).trim().toLowerCase();
          const value = line.slice(colon + 1).trim();
          allHeaders[name] = value;
        }
        const tracked = {};
        for (const name of TRACKED_RESPONSE_HEADERS) {
          if (name in allHeaders) tracked[name] = allHeaders[name];
        }
        const body = summarizeBody(bodyBuf, tracked['content-type'] ?? '');
        resolveFn({
          name: req.name,
          mode: 'raw',
          method,
          path,
          requestHeaders: req.headers ?? {},
          requestLine: requestLineCapture,
          status,
          statusText,
          headers: tracked,
          body,
        });
      } catch (e) {
        rejectFn(e);
      }
    });
  });
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
    if (r.mode === 'raw' && r.requestLine) {
      lines.push(`- Mode: \`raw\` (literal HTTP request line over net.Socket)`);
      lines.push(`- Wire request line: \`${r.requestLine}\``);
    }
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

// ---------------------------------------------------------------------------
// Snapshot helpers
// ---------------------------------------------------------------------------

function sortObject(obj) {
  if (obj === null || typeof obj !== 'object' || Array.isArray(obj)) return obj;
  const out = {};
  for (const k of Object.keys(obj).sort()) out[k] = obj[k];
  return out;
}

function normalizeRequestEntry(probe, result) {
  const requestHeaders = sortObject(result.requestHeaders ?? {});
  const respHeaders = sortObject(result.headers ?? {});
  const bodyKind =
    result.body.preview !== null && result.body.preview !== undefined
      ? 'text'
      : result.body.length === 0
        ? 'empty'
        : 'binary';
  const body = {
    kind: bodyKind,
    length: result.body.length,
    sha256: result.body.sha256,
  };
  if (result.body.preview !== null && result.body.preview !== undefined) {
    body.preview = result.body.preview;
  }
  const entry = {
    name: result.name,
    request: {
      method: result.method,
      mode: result.mode,
      path: result.path,
    },
  };
  if (Object.keys(requestHeaders).length) entry.request.headers = requestHeaders;
  if (result.mode === 'raw' && result.requestLine) {
    entry.requestLine = result.requestLine;
  }
  entry.response = {
    body,
    headers: respHeaders,
    status: result.status,
  };
  if (result.statusText) entry.response.statusText = result.statusText;
  return entry;
}

function buildSnapshot(probe, results) {
  const volatileHeaders =
    probe.snapshot?.volatileHeaders ?? [...DEFAULT_VOLATILE_HEADERS];
  const volatileBodies = new Set(probe.snapshot?.volatileBodies ?? []);
  const entries = results.map((r) => normalizeRequestEntry(probe, r));
  // For requests flagged as volatile-body, redact the body shape on disk
  // (keep only `kind`). We do this at write time so the committed snapshot
  // is reproducible across checkouts — otherwise an absolute fixture path
  // leaking into a directory listing's preview would churn whenever a
  // contributor regenerates snapshots.
  for (const e of entries) {
    if (volatileBodies.has(e.name) && e.response?.body) {
      e.response.body = { kind: e.response.body.kind };
    }
  }
  const snap = {
    $schemaVersion: 1,
    case: probe.id,
    description: probe.description ?? null,
    fixture: probe.fixture ?? null,
    serveArgs: probe.serveArgs ?? [],
    volatileHeaders: [...volatileHeaders].sort(),
    volatileBodies: [...volatileBodies].sort(),
    requests: entries,
  };
  return snap;
}

function maskVolatileForDiff(snap) {
  const volatileHeaders = new Set(snap.volatileHeaders ?? []);
  const volatileBodies = new Set(snap.volatileBodies ?? []);
  const cloned = JSON.parse(JSON.stringify(snap));
  for (const req of cloned.requests ?? []) {
    const h = req.response?.headers ?? {};
    for (const k of Object.keys(h)) {
      if (volatileHeaders.has(k)) h[k] = '<volatile>';
    }
    if (volatileBodies.has(req.name) && req.response?.body) {
      const b = req.response.body;
      if ('length' in b) b.length = '<volatile>';
      if ('sha256' in b) b.sha256 = '<volatile>';
      if ('preview' in b) b.preview = '<volatile>';
    }
  }
  return cloned;
}

function canonicalJson(value) {
  // Stable JSON: sort all object keys recursively.
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  if (Array.isArray(value)) {
    return '[' + value.map(canonicalJson).join(',') + ']';
  }
  const keys = Object.keys(value).sort();
  return '{' + keys.map((k) => JSON.stringify(k) + ':' + canonicalJson(value[k])).join(',') + '}';
}

function diffSnapshots(expected, actual) {
  // Mask volatile headers using the expected snapshot's volatile list (the
  // committed contract). Then compare canonicalized JSON.
  const exp = maskVolatileForDiff(expected);
  const act = maskVolatileForDiff({
    ...actual,
    volatileHeaders: expected.volatileHeaders ?? [],
  });
  if (canonicalJson(exp) === canonicalJson(act)) return null;
  // Produce a readable line-based diff of the prettified forms so a reviewer
  // can locate the divergence quickly.
  const expS = JSON.stringify(exp, null, 2).split('\n');
  const actS = JSON.stringify(act, null, 2).split('\n');
  const max = Math.max(expS.length, actS.length);
  const out = [];
  for (let i = 0; i < max; i++) {
    const e = expS[i] ?? '';
    const a = actS[i] ?? '';
    if (e !== a) {
      out.push(`L${i + 1}:`);
      out.push(`  expected: ${e}`);
      out.push(`  actual:   ${a}`);
    }
  }
  return out.join('\n');
}

async function readSnapshot(probeId) {
  const path = join(SNAPSHOTS_DIR, `${probeId}.json`);
  try {
    const raw = await readFile(path, 'utf8');
    return JSON.parse(raw);
  } catch (e) {
    if (e.code === 'ENOENT') return null;
    throw e;
  }
}

async function writeSnapshot(probeId, snapshot) {
  await mkdir(SNAPSHOTS_DIR, { recursive: true });
  const text = JSON.stringify(snapshot, null, 2) + '\n';
  await writeFile(join(SNAPSHOTS_DIR, `${probeId}.json`), text);
}

async function snapshotExists(probeId) {
  try {
    await access(join(SNAPSHOTS_DIR, `${probeId}.json`));
    return true;
  } catch {
    return false;
  }
}

async function readServeVersion() {
  try {
    const pkg = JSON.parse(await readFile(join(REPO_ROOT, 'third_party', 'serve', 'package.json'), 'utf8'));
    return pkg.version;
  } catch {
    return null;
  }
}

async function runProbe(probeId, options = {}) {
  const { snapshotMode = 'auto' } = options;
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
  // Resolve the effective snapshot mode. `auto` means: verify if a snapshot
  // exists, else `none`. The other modes (`update`, `verify`, `none`) are
  // taken as-is.
  let effectiveMode = snapshotMode;
  if (effectiveMode === 'auto') {
    effectiveMode = (await snapshotExists(probeId)) ? 'verify' : 'none';
  }
  try {
    await materializeFixture(probe.fixture ?? {}, fixtureDir);
    port = await getFreePort();
    runMeta.port = port;
    logInfo(`probe '${probeId}': serve@${runMeta.serveVersion} on port ${port}, fixture=${relative(REPO_ROOT, fixtureDir)}, snapshot=${effectiveMode}`);
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

    // Snapshot handling.
    let snapshotStatus = 'skipped';
    if (effectiveMode === 'update') {
      const snap = buildSnapshot(probe, results);
      await writeSnapshot(probeId, snap);
      snapshotStatus = 'updated';
    } else if (effectiveMode === 'verify') {
      const expected = await readSnapshot(probeId);
      if (!expected) {
        throw new Error(`--snapshot=verify but no snapshot at snapshots/${probeId}.json (run with --snapshot=update first)`);
      }
      const actual = buildSnapshot(probe, results);
      const diff = diffSnapshots(expected, actual);
      if (diff) {
        const err = new Error(`snapshot mismatch for ${probeId}\n${diff}`);
        err.snapshotMismatch = true;
        throw err;
      }
      snapshotStatus = 'verified';
    }

    logInfo(`probe '${probeId}': results/${probeId}.{md,json} written (${results.length} requests, snapshot=${snapshotStatus})`);
    return { probeId, requests: results.length, snapshot: snapshotStatus };
  } finally {
    if (serveProc) await killServe(serveProc);
    if (serveBundle && process.env.PROBE_VERBOSE) {
      const stderr = serveBundle.getStderr();
      if (stderr.trim()) logInfo(`probe '${probeId}' serve stderr:\n${stderr}`);
    }
    await rm(fixtureDir, { recursive: true, force: true });
  }
}

function parseArgs(argv) {
  const positional = [];
  let snapshotMode = 'auto';
  for (const a of argv) {
    if (a.startsWith('--snapshot=')) {
      const v = a.slice('--snapshot='.length);
      if (!['update', 'verify', 'none', 'auto'].includes(v)) {
        throw new Error(`invalid --snapshot value: ${v} (expected update | verify | none | auto)`);
      }
      snapshotMode = v;
    } else {
      positional.push(a);
    }
  }
  return { positional, snapshotMode };
}

async function main() {
  const rawArgs = process.argv.slice(2);
  if (rawArgs.length === 0 || rawArgs.includes('--help') || rawArgs.includes('-h')) {
    process.stdout.write([
      'Usage:',
      '  node tools/probe/run.mjs <probe-id> [--snapshot=update|verify|none]',
      '  node tools/probe/run.mjs --all       [--snapshot=update|verify|none]',
      '  node tools/probe/run.mjs --list',
      '',
      'Case files live in tools/probe/cases/<probe-id>.json.',
      'Results land in tools/probe/results/<probe-id>.{md,json} (gitignored).',
      'Snapshots live in tools/probe/snapshots/<probe-id>.json (committed).',
      '',
      'Snapshot modes:',
      '  update  — capture/refresh snapshots/<id>.json from the current run.',
      '  verify  — diff the current run against snapshots/<id>.json (default',
      '            when a snapshot exists; non-zero exit on mismatch).',
      '  none    — skip snapshot handling (legacy, default when no snapshot).',
      '',
      'Set PROBE_VERBOSE=1 to also print serve stderr after each run.',
      '',
    ].join('\n'));
    process.exit(rawArgs.length === 0 ? 1 : 0);
  }
  const { positional, snapshotMode } = parseArgs(rawArgs);
  if (positional[0] === '--list') {
    const ids = await listCases();
    for (const id of ids) process.stdout.write(`${id}\n`);
    return;
  }
  let ids;
  if (positional[0] === '--all') {
    ids = await listCases();
  } else {
    ids = positional;
  }
  let failed = 0;
  let mismatched = 0;
  for (const id of ids) {
    try {
      await runProbe(id, { snapshotMode });
    } catch (e) {
      failed++;
      if (e.snapshotMismatch) mismatched++;
      logError(`probe '${id}': ${e.message}`);
      if (process.env.PROBE_VERBOSE) logError(e.stack ?? '');
    }
  }
  if (failed > 0) {
    logError(`${failed} of ${ids.length} probes failed${mismatched ? ` (${mismatched} snapshot mismatches)` : ''}`);
    process.exit(1);
  }
}

main().catch((e) => {
  logError(e.stack ?? String(e));
  process.exit(2);
});
