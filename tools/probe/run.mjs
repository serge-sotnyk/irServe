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

// Extra volatile headers applied on top of the case's `volatileHeaders` set
// when running against the strict L0 Rust runtime (`target=irserve`). The
// reference (vercel/serve) emits these unconditionally; irserve omits them
// (or differs on them) by deliberate L0 design (D-008/D-003 etc.). They are
// masked from BOTH sides of the diff so the comparison stays meaningful.
const L0_EXTRA_VOLATILE_HEADERS = ['etag', 'vary', 'accept-ranges'];

// Flags deferred from the strict L0 implementation (D-008). If a case asks
// for one of these via `serveArgs`, the runner refuses to run it against
// `target=irserve` rather than silently dropping the flag.
// - `-c`/`--config` un-deferred in stage 6a (change 003-load-serve-json).
// - `-s`/`--single` un-deferred in stage 6e (change 007-configured-rewrites,
//   D-013).
// - `-p`, `tcp://` listen URI un-deferred in stage 6h (change 010-cli-fill-in).
// - `-C`/`--cors` un-deferred in stage 6h (change 010-cli-fill-in).
const L0_DEFERRED_FLAGS = new Set([
  '-d', '--debug',
  '-L', '--no-request-logging',
  '--no-port-switching',
]);

function resolveIrserveBin() {
  if (process.env.IRSERVE_BIN) return process.env.IRSERVE_BIN;
  const exe = process.platform === 'win32' ? 'irserve.exe' : 'irserve';
  return join(REPO_ROOT, 'target', 'debug', exe);
}

const IRSERVE_BIN = resolveIrserveBin();

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

// Detect whether `extraArgs` (case-level `serveArgs`) contains any flag that
// the strict L0 Rust runtime rejects. Returns null when clean, or a string
// describing the offending flag when not.
function findDeferredL0Flag(extraArgs) {
  const args = extraArgs ?? [];
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (L0_DEFERRED_FLAGS.has(a)) return a;
  }
  return null;
}

function spawnServe({ port, fixtureDir, extraArgs, target = 'reference', skipAutoListen = false, extraEnv = {} }) {
  const isIrserve = target === 'irserve';
  const baseEnv = { ...process.env, NO_UPDATE_CHECK: '1', FORCE_COLOR: '0', ...extraEnv };
  // `{port}` substitution lets cases that own their own --listen value
  // (e.g. `--listen tcp://127.0.0.1:{port}` or `-p {port}`) reference the
  // runner-allocated free port without hard-coding it.
  const substitutedExtra = (extraArgs ?? []).map((a) =>
    typeof a === 'string' ? a.replace(/\{port\}/g, String(port)) : a,
  );
  let cmd;
  let args;
  if (isIrserve) {
    const injected = ['--no-clipboard'];
    if (!skipAutoListen) injected.unshift('--listen', String(port));
    args = [...injected, ...substitutedExtra, fixtureDir];
    cmd = IRSERVE_BIN;
  } else {
    const injected = ['--no-clipboard', '--no-port-switching'];
    if (!skipAutoListen) injected.unshift('--listen', String(port));
    args = [SERVE_ENTRY, ...injected, ...substitutedExtra, fixtureDir];
    cmd = process.execPath;
  }
  const child = spawn(cmd, args, {
    cwd: REPO_ROOT,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: baseEnv,
  });
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', (d) => { stdout += d.toString(); });
  child.stderr.on('data', (d) => { stderr += d.toString(); });
  return { child, getStdout: () => stdout, getStderr: () => stderr };
}

// Verify port 3000 is free for `defaultPortScenario=no-flag-no-env`. We try
// to listen on 127.0.0.1:3000 and then close: if the bind succeeds, the port
// was free at this instant. There's an unavoidable race between this check
// and the spawned server's own bind, but it's no worse than getFreePort().
async function ensurePort3000Free(probeId) {
  return new Promise((resolveFn, rejectFn) => {
    const srv = net.createServer();
    srv.unref();
    srv.once('error', () => {
      rejectFn(new Error(`port 3000 occupied; cannot run case "${probeId}" with defaultPortScenario=no-flag-no-env`));
    });
    srv.listen(3000, '127.0.0.1', () => {
      srv.close(() => resolveFn());
    });
  });
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

// Normalize the bytes of a response body so the recorded body sha256 / preview
// stay reproducible across machines. The pinned reference embeds the absolute
// fixture directory (e.g. directory listings, JSON listings) which is a
// per-run mkdtemp path. We replace it with stable tokens.
//
// Replacements applied (longest-first, so basename does not eat the full path):
//   <fixture-dir-json-escaped>   -> "<FIXTURE_ROOT>"
//   <fixture-dir-verbatim>       -> "<FIXTURE_ROOT>"
//   <fixture-dir-forward-slash>  -> "<FIXTURE_ROOT>"
//   <basename(fixture-dir)>      -> "<FIXTURE_BASENAME>"
function normalizeBodyForSnapshot(buffer, fixtureDir) {
  if (!fixtureDir) return buffer;
  const text = buffer.toString('binary');
  const baseName = fixtureDir.split(/[\\/]/).filter(Boolean).pop() || '';
  const variants = [
    fixtureDir.replace(/\\/g, '\\\\'),
    fixtureDir,
    fixtureDir.replace(/\\/g, '/'),
  ].filter((v, i, a) => v && a.indexOf(v) === i);
  let out = text;
  for (const v of variants) out = out.split(v).join('<FIXTURE_ROOT>');
  if (baseName && baseName !== '<FIXTURE_ROOT>') {
    out = out.split(baseName).join('<FIXTURE_BASENAME>');
  }
  return Buffer.from(out, 'binary');
}

// Derive a stream/body's `kind` from its summary. Length wins: a zero-byte
// stream is always `empty`, regardless of the declared content-type. This
// is the assertion CLI verify relies on (with length/sha/preview masked,
// kind is the only thing that distinguishes empty stderr from non-empty).
function deriveKind({ length, preview }) {
  if (length === 0) return 'empty';
  if (preview !== null && preview !== undefined) return 'text';
  return 'binary';
}

function summarizeBody(buffer, contentType = '', fixtureDir = null) {
  const normalized = normalizeBodyForSnapshot(buffer, fixtureDir);
  const sha256 = createHash('sha256').update(normalized).digest('hex');
  const length = normalized.length;
  // An empty buffer never has a preview, even if the declared content-type
  // is textual. This is what callers downstream rely on to derive a
  // `kind: "empty"` body shape (vs `text` or `binary`).
  if (length === 0) return { length, sha256, preview: null };
  const isTextual = /^(text\/|application\/(json|xml|javascript)|.*\+(json|xml))/i.test(contentType);
  let preview = null;
  if (isTextual) {
    const slice = normalized.slice(0, 200);
    preview = slice.toString('utf8');
  }
  return { length, sha256, preview };
}

function pickHeaders(headers, extraTracked = []) {
  // headers may be either a Headers instance (fetch path) or a plain object
  // (raw-socket path; keys are already lowercased).
  const allowlist = new Set([...TRACKED_RESPONSE_HEADERS, ...extraTracked.map((n) => n.toLowerCase())]);
  const out = {};
  if (headers && typeof headers.get === 'function') {
    for (const name of allowlist) {
      const v = headers.get(name);
      if (v !== null && v !== undefined) out[name] = v;
    }
  } else if (headers && typeof headers === 'object') {
    for (const [k, v] of Object.entries(headers)) {
      if (allowlist.has(k.toLowerCase())) out[k.toLowerCase()] = v;
    }
  }
  return out;
}

async function runRequest(port, req, opts = {}) {
  const { fixtureDir = null, extraTracked = [] } = opts;
  if (req.mode === 'raw') return runRequestRaw(port, req, opts);
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
  const headers = pickHeaders(res.headers, extraTracked);
  const body = summarizeBody(buf, headers['content-type'], fixtureDir);
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
async function runRequestRaw(port, req, opts = {}) {
  const { fixtureDir = null, extraTracked = [] } = opts;
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
        const tracked = pickHeaders(allHeaders, extraTracked);
        const body = summarizeBody(bodyBuf, tracked['content-type'] ?? '', fixtureDir);
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

function renderMarkdown(probe, runMeta, results, opts = {}) {
  const { l0 = null } = opts;
  const divergentSet = new Set(l0?.divergent ?? []);
  const cleanResults = l0 ? results.filter((r) => !divergentSet.has(r.name)) : results;
  const divergentResults = l0 ? results.filter((r) => divergentSet.has(r.name)) : [];
  const lines = [];
  lines.push(`# Probe \`${probe.id}\``);
  lines.push('');
  if (probe.description) {
    lines.push(probe.description);
    lines.push('');
  }
  lines.push('## Run metadata');
  lines.push('');
  if (runMeta.target) lines.push(`- Target: \`${runMeta.target}\``);
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
  for (const r of cleanResults) {
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
  if (divergentResults.length > 0) {
    lines.push('### Informational (L1-divergent against strict L0)');
    lines.push('');
    lines.push('The following anchors were executed against the target but excluded');
    lines.push('from the snapshot diff because the case marks them as L1-divergent.');
    lines.push('');
    for (const r of divergentResults) {
      lines.push(`#### \`${r.name}\` (informational)`);
      lines.push('');
      lines.push(`- Request: \`${r.method} ${r.path}\``);
      lines.push(`- Status: \`${r.status} ${r.statusText}\``);
      if (Object.keys(r.headers).length) {
        lines.push(`- Response headers:`);
        for (const [k, v] of Object.entries(r.headers)) {
          lines.push(`  - \`${k}: ${v}\``);
        }
      }
      lines.push(`- Body length: \`${r.body.length}\``);
      lines.push(`- Body sha256: \`${r.body.sha256}\``);
      lines.push('');
    }
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
  const body = {
    kind: deriveKind(result.body),
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
  const volatileBodies = probe.snapshot?.volatileBodies ?? [];
  const entries = results.map((r) => normalizeRequestEntry(probe, r));
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

function buildCliSnapshot(probe, cliResults) {
  return {
    $schemaVersion: 1,
    case: probe.id,
    description: probe.description ?? null,
    cli: cliResults.map((r) => ({
      name: r.name,
      args: r.args,
      exitCode: r.exitCode,
      stdout: r.stdout,
      stderr: r.stderr,
    })),
  };
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
  // CLI snapshots: mask stdout/stderr length/sha256/preview by default.
  // The exact terminal output is excluded by `D-002` (decisions.md), so
  // verify asserts only exitCode + stream `kind` (text/empty/binary). The
  // full byte-level data is still recorded on disk for audit.
  for (const inv of cloned.cli ?? []) {
    for (const stream of ['stdout', 'stderr']) {
      const s = inv[stream];
      if (!s) continue;
      if ('length' in s) s.length = '<volatile>';
      if ('sha256' in s) s.sha256 = '<volatile>';
      if ('preview' in s) s.preview = '<volatile>';
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

// L0 mode preprocessor: when running against irserve, apply the case's
// per-anchor L0 partition to BOTH sides of the snapshot before diffing.
//   * remove `divergent` anchors entirely (informational only)
//   * for `bodyMayDiffer` anchors, strip body.kind / body.length / body.sha256 /
//     body.preview (and content-length, since body length implies content-length).
//     `body.kind` is included because raw-mode probes capture HTTP/1.1
//     chunked-encoding terminators (`0\r\n\r\n`) as a 5-byte "binary" body
//     against Node's reference, while irserve's hyper layer uses
//     `Content-Length: 0` for the same empty 301 — a transport-encoding
//     choice, not an application-body divergence.
//   * for `contentLengthMayDiffer` anchors, strip only the content-length
//     header (transport detail; body bytes still must-match)
//   * for `exitCodeMayDiffer` anchors (CLI entries), assert both sides are
//     non-zero then strip exitCode (per ORC-062 must-match: "non-zero")
//   * extend volatileHeaders with L0_EXTRA_VOLATILE_HEADERS so the masking
//     pass blanks them out on both sides.
function applyL0Filter(snap, l0) {
  if (!l0) return snap;
  const cloned = JSON.parse(JSON.stringify(snap));
  const clean = new Set(l0.clean ?? []);
  const divergent = new Set(l0.divergent ?? []);
  const bodyMayDiffer = new Set(l0.bodyMayDiffer ?? []);
  const contentLengthMayDiffer = new Set(l0.contentLengthMayDiffer ?? []);
  const exitCodeMayDiffer = new Set(l0.exitCodeMayDiffer ?? []);
  // The L0 partition is a strict allow-list: only `clean` anchors are
  // compared. `divergent` is informational. Anchors absent from both lists
  // are stripped from the diff (informational). This makes case files
  // self-documenting about what's contractually asserted. An empty `clean`
  // would make the case vacuously pass — guarded by the post-filter check
  // below.
  const inClean = (name) => clean.has(name);
  if (Array.isArray(cloned.requests)) {
    const beforeCount = cloned.requests.length;
    cloned.requests = cloned.requests.filter((r) => !divergent.has(r.name) && inClean(r.name));
    if (cloned.requests.length === 0 && beforeCount > 0) {
      throw new Error(
        `L0 filter: case has zero anchors in 'clean' (or all anchors are divergent). ` +
          `An empty must-match set would make the case vacuously pass. ` +
          `Either populate runner.l0.clean or remove the runner.l0 block (which skips the case under target=irserve).`
      );
    }
    for (const r of cloned.requests) {
      // Strip L0 extra-volatile headers entirely from both sides. We can't
      // use the existing `<volatile>` masking because if a header is present
      // on one side but missing on the other, the masked-vs-missing diff
      // still fires.
      if (r.response?.headers) {
        for (const h of L0_EXTRA_VOLATILE_HEADERS) {
          delete r.response.headers[h];
        }
      }
      if (bodyMayDiffer.has(r.name) && r.response?.body) {
        const b = r.response.body;
        if ('kind' in b) delete b.kind;
        if ('length' in b) delete b.length;
        if ('sha256' in b) delete b.sha256;
        if ('preview' in b) delete b.preview;
        // The body byte count is what content-length reports, so when body
        // is may-differ, content-length must also be may-differ.
        if (r.response.headers && 'content-length' in r.response.headers) {
          delete r.response.headers['content-length'];
        }
      }
      if (contentLengthMayDiffer.has(r.name) && r.response?.headers) {
        delete r.response.headers['content-length'];
      }
    }
  }
  if (Array.isArray(cloned.cli)) {
    const beforeCount = cloned.cli.length;
    cloned.cli = cloned.cli.filter((e) => !divergent.has(e.name) && inClean(e.name));
    if (cloned.cli.length === 0 && beforeCount > 0) {
      throw new Error(
        `L0 filter: CLI case has zero anchors in 'clean' (or all anchors are divergent). ` +
          `An empty must-match set would make the case vacuously pass. ` +
          `Either populate runner.l0.clean or remove the runner.l0 block (which skips the case under target=irserve).`
      );
    }
    for (const e of cloned.cli) {
      if (exitCodeMayDiffer.has(e.name)) {
        // Per ORC-062 must-match the exit code is "non-zero", not a specific
        // value. Refuse to strip a zero exit code — that would be a real
        // contract violation.
        if (typeof e.exitCode === 'number' && e.exitCode === 0) {
          throw new Error(
            `L0 filter: anchor "${e.name}" has exitCode=0 but is in exitCodeMayDiffer; ` +
              `non-zero is contractual (ORC-062). Cannot strip.`
          );
        }
        delete e.exitCode;
      }
    }
  }
  return cloned;
}

function diffSnapshots(expected, actual, opts = {}) {
  const { target = 'reference', l0 = null } = opts;
  // Mask volatile headers/bodies using the expected snapshot's volatile
  // lists (the committed contract). CLI streams are masked unconditionally
  // — see maskVolatileForDiff for rationale. Compare canonicalized JSON.
  const expFiltered = target === 'irserve' ? applyL0Filter(expected, l0) : expected;
  const actFiltered = target === 'irserve' ? applyL0Filter(actual, l0) : actual;
  const exp = maskVolatileForDiff(expFiltered);
  const overlay = {};
  if ('volatileHeaders' in expFiltered) overlay.volatileHeaders = expFiltered.volatileHeaders;
  if ('volatileBodies' in expFiltered) overlay.volatileBodies = expFiltered.volatileBodies;
  const act = maskVolatileForDiff({ ...actFiltered, ...overlay });
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

// Run a CLI probe: spawn `node serve.js <args>` to completion (no port, no
// HTTP). Capture exitCode + stdout + stderr (each summarized as
// {kind, length, sha256, preview}). Used for cases asserting --help,
// --version, two-positional-arg failure, etc.
async function runCliInvocation(name, args, fixtureDir, target = 'reference') {
  const isIrserve = target === 'irserve';
  const cmd = isIrserve ? IRSERVE_BIN : process.execPath;
  const cmdArgs = isIrserve ? [...args] : [SERVE_ENTRY, ...args];
  return new Promise((resolveFn, rejectFn) => {
    const child = spawn(cmd, cmdArgs, {
      cwd: fixtureDir,
      stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...process.env, NO_UPDATE_CHECK: '1', FORCE_COLOR: '0' },
    });
    const stdoutChunks = [];
    const stderrChunks = [];
    child.stdout.on('data', (d) => stdoutChunks.push(d));
    child.stderr.on('data', (d) => stderrChunks.push(d));
    const timer = setTimeout(() => {
      try { child.kill('SIGKILL'); } catch { /* ignore */ }
      rejectFn(new Error(`CLI invocation '${name}' timed out`));
    }, 15000);
    child.on('error', (e) => { clearTimeout(timer); rejectFn(e); });
    child.on('close', (exitCode) => {
      clearTimeout(timer);
      const stdoutBuf = Buffer.concat(stdoutChunks);
      const stderrBuf = Buffer.concat(stderrChunks);
      const stdout = summarizeBody(stdoutBuf, 'text/plain', fixtureDir);
      const stderr = summarizeBody(stderrBuf, 'text/plain', fixtureDir);
      const wrap = (s) => ({
        kind: deriveKind(s),
        length: s.length,
        sha256: s.sha256,
        ...(s.preview !== null ? { preview: s.preview } : {}),
      });
      resolveFn({
        name,
        args,
        exitCode,
        stdout: wrap(stdout),
        stderr: wrap(stderr),
      });
    });
  });
}

async function runCliProbe(probeId, probe, snapshotMode, target = 'reference') {
  await mkdir(TMP_DIR, { recursive: true });
  const fixtureDir = await mkdtemp(join(TMP_DIR, `${probeId}-`));
  const l0 = probe.runner?.l0 ?? null;
  try {
    await materializeFixture(probe.fixture ?? {}, fixtureDir);
    let effectiveMode = snapshotMode;
    if (effectiveMode === 'auto') {
      effectiveMode = (await snapshotExists(probeId)) ? 'verify' : 'none';
    }
    logInfo(`probe '${probeId}': target=${target} cli mode (${probe.cli.length} invocation(s)), snapshot=${effectiveMode}`);
    const cliResults = [];
    for (const inv of probe.cli) {
      cliResults.push(await runCliInvocation(inv.name, inv.args ?? [], fixtureDir, target));
    }

    let snapshotStatus = 'skipped';
    if (effectiveMode === 'update') {
      await writeSnapshot(probeId, buildCliSnapshot(probe, cliResults));
      snapshotStatus = 'updated';
    } else if (effectiveMode === 'verify') {
      const expected = await readSnapshot(probeId);
      if (!expected) {
        throw new Error(`--snapshot=verify but no snapshot at snapshots/${probeId}.json (run with --snapshot=update first)`);
      }
      const actual = buildCliSnapshot(probe, cliResults);
      const diff = diffSnapshots(expected, actual, { target, l0 });
      if (diff) {
        const err = new Error(`snapshot mismatch for ${probeId}\n${diff}`);
        err.snapshotMismatch = true;
        throw err;
      }
      snapshotStatus = 'verified';
    }
    logInfo(`probe '${probeId}': cli mode (${cliResults.length} invocation(s), snapshot=${snapshotStatus})`);
    return { probeId, requests: cliResults.length, snapshot: snapshotStatus };
  } finally {
    await rm(fixtureDir, { recursive: true, force: true });
  }
}

async function runProbe(probeId, options = {}) {
  const { snapshotMode = 'auto', target = 'reference' } = options;
  const probe = await readCase(probeId);
  if (!probe.id) probe.id = probeId;
  const l0 = probe.runner?.l0 ?? null;
  // §D rule 3: cases with no L0 partition declared are skipped against irserve.
  if (target === 'irserve' && !l0) {
    logInfo(`probe '${probeId}': skipped (no L0 partition declared)`);
    return { probeId, skipped: true, reason: 'no L0 partition declared' };
  }
  // CLI mode: case has a `cli` array instead of `requests`.
  if (Array.isArray(probe.cli) && probe.cli.length > 0) {
    return runCliProbe(probeId, probe, snapshotMode, target);
  }
  // L0: refuse cases with deferred flags rather than silently dropping them.
  if (target === 'irserve') {
    const offending = findDeferredL0Flag(probe.serveArgs ?? []);
    if (offending) {
      throw new Error(`case "${probeId}" uses deferred flag ${offending}; cannot run against target=irserve (D-008)`);
    }
  }
  await mkdir(RESULTS_DIR, { recursive: true });
  await mkdir(TMP_DIR, { recursive: true });
  const fixtureDir = await mkdtemp(join(TMP_DIR, `${probeId}-`));
  let port;
  let serveProc = null;
  let serveBundle = null;
  const runMeta = {
    generatedAt: new Date().toISOString(),
    serveVersion: await readServeVersion(),
    target,
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

    // §C: resolve port + spawn options based on per-case skipAutoListen flag.
    const skipAutoListen = probe.runner?.skipAutoListen === true;
    const defaultPortScenario = probe.runner?.defaultPortScenario ?? null;
    const extraEnv = {};
    if (skipAutoListen) {
      if (!defaultPortScenario) {
        throw new Error(`case "${probeId}": runner.skipAutoListen=true requires runner.defaultPortScenario`);
      }
      if (defaultPortScenario === 'no-flag-no-env') {
        await ensurePort3000Free(probeId);
        port = 3000;
      } else if (defaultPortScenario === 'env-var') {
        const allocated = await getFreePort();
        extraEnv.PORT = String(allocated);
        port = allocated;
      } else if (defaultPortScenario === 'free-port') {
        // The case owns its own --listen value (e.g. `tcp://127.0.0.1:{port}`
        // or `-p {port}`). The runner allocates a free port and the case's
        // serveArgs reference it via the `{port}` substitution.
        port = await getFreePort();
      } else {
        throw new Error(`case "${probeId}": unknown runner.defaultPortScenario "${defaultPortScenario}"`);
      }
    } else {
      port = await getFreePort();
    }
    runMeta.port = port;
    logInfo(`probe '${probeId}': target=${target} on port ${port}, fixture=${relative(REPO_ROOT, fixtureDir)}, snapshot=${effectiveMode}`);
    serveBundle = spawnServe({
      port,
      fixtureDir,
      extraArgs: probe.serveArgs ?? [],
      target,
      skipAutoListen,
      extraEnv,
    });
    serveProc = serveBundle.child;
    await waitReady(port);
    const extraTracked = probe.snapshot?.extraTrackedHeaders ?? [];
    const requestOpts = { fixtureDir, extraTracked };
    const results = [];
    for (const req of probe.requests ?? []) {
      results.push(await runRequest(port, req, requestOpts));
    }
    const md = renderMarkdown(probe, runMeta, results, { l0: target === 'irserve' ? l0 : null });
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
      const diff = diffSnapshots(expected, actual, { target, l0 });
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
  let target = null;
  for (const a of argv) {
    if (a.startsWith('--snapshot=')) {
      const v = a.slice('--snapshot='.length);
      if (!['update', 'verify', 'none', 'auto'].includes(v)) {
        throw new Error(`invalid --snapshot value: ${v} (expected update | verify | none | auto)`);
      }
      snapshotMode = v;
    } else if (a.startsWith('--target=')) {
      const v = a.slice('--target='.length);
      if (!['reference', 'irserve'].includes(v)) {
        throw new Error(`invalid --target value: ${v} (expected reference | irserve)`);
      }
      target = v;
    } else {
      positional.push(a);
    }
  }
  // CLI flag wins; PROBE_TARGET env var is the fallback. Default: 'reference'.
  if (target === null) {
    const envTarget = process.env.PROBE_TARGET;
    if (envTarget) {
      if (!['reference', 'irserve'].includes(envTarget)) {
        throw new Error(`invalid PROBE_TARGET env value: ${envTarget} (expected reference | irserve)`);
      }
      target = envTarget;
    } else {
      target = 'reference';
    }
  }
  return { positional, snapshotMode, target };
}

async function main() {
  const rawArgs = process.argv.slice(2);
  if (rawArgs.length === 0 || rawArgs.includes('--help') || rawArgs.includes('-h')) {
    process.stdout.write([
      'Usage:',
      '  node tools/probe/run.mjs <probe-id> [--snapshot=update|verify|none] [--target=reference|irserve]',
      '  node tools/probe/run.mjs --all       [--snapshot=update|verify|none] [--target=reference|irserve]',
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
      'Targets:',
      '  reference — pinned `vercel/serve` Node bundle (default).',
      '  irserve   — strict L0 Rust binary at $IRSERVE_BIN (or target/debug/irserve).',
      '              Cases without a `runner.l0` partition are skipped.',
      '              Set via --target=… or PROBE_TARGET=…; CLI flag wins.',
      '',
      'Set PROBE_VERBOSE=1 to also print serve stderr after each run.',
      '',
    ].join('\n'));
    process.exit(rawArgs.length === 0 ? 1 : 0);
  }
  const { positional, snapshotMode, target } = parseArgs(rawArgs);
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
  let skipped = 0;
  let passed = 0;
  for (const id of ids) {
    try {
      const result = await runProbe(id, { snapshotMode, target });
      if (result?.skipped) skipped++;
      else passed++;
    } catch (e) {
      failed++;
      if (e.snapshotMismatch) mismatched++;
      logError(`probe '${id}': ${e.message}`);
      if (process.env.PROBE_VERBOSE) logError(e.stack ?? '');
    }
  }
  logInfo(`summary: target=${target} total=${ids.length} passed=${passed} skipped=${skipped} failed=${failed}`);
  if (failed > 0) {
    logError(`${failed} of ${ids.length} probes failed${mismatched ? ` (${mismatched} snapshot mismatches)` : ''}`);
    process.exit(1);
  }
}

main().catch((e) => {
  logError(e.stack ?? String(e));
  process.exit(2);
});
