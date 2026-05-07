# Decisions log

Intentional deviations from `vercel/serve` behavior (`adapted` status) and explicit non-goals (`rejected` status). Each entry must reference the affected requirement ID(s) and explain the rationale.

## Format

```
## D-<NNN>: <short title>

Date: YYYY-MM-DD
Affected requirements: SRV-<AREA>-<NNN>, ...
Status: adapted | rejected
Reason: <why we deviate>
Impact: <what users / oracle tests should expect>
```

## Entries

## D-001: No Node.js middleware API

Date: 2026-05-07
Affected requirements: (none — entire `serve-handler` API surface)
Status: rejected
Reason: IrServe is a CLI binary written in Rust. The Node-specific `handler(request, response, config, methods)` middleware API has no analog in a Rust HTTP server, and exposing an embeddable library is outside the MVP scope per anti-hallucination rule #5.
Impact: IrServe MUST NOT be used as a drop-in replacement for `serve-handler` in Node code paths. The configuration *file format* (`serve.json`) is in scope; the *programmatic API* is not.

## D-002: No exact terminal output / stdout formatting

Date: 2026-05-07
Affected requirements: SRV-CLI-014 (`-d`/`--debug`), SRV-CLI-015 (`-L`/`--no-request-logging`), SRV-CLI-016 (port-switching warning), SRV-CLI-019 (`--help`/`--version`)
Status: rejected
Reason: The `serve` CLI uses `chalk`, `boxen`, and a specific log format (date prefix, IP, status, ms-elapsed). Mirroring this byte-for-byte adds churn without functional value and is excluded by anti-hallucination rule #5.
Impact: Oracle tests MUST NOT compare stdout/stderr text. Verbosity flags (`-d`, `-L`) are accepted; their effect on logging is implementation-defined.

## D-003: No exact HTML/CSS markup of the directory listing or error pages

Date: 2026-05-07
Affected requirements: SRV-DLST-001, SRV-FILE-002 (HTML branch), SRV-FILE-003 (HTML branch)
Status: rejected
Reason: The directory listing and the default error page are styled HTML templates inside `serve-handler`. Their visual design is not a contractual interface; only the *existence*, status code, and `Content-Type` of the response are. (See anti-hallucination rule #5.)
Impact: Oracle tests MUST NOT compare HTML body bytes for listings or for the no-`<status>.html` error path. The JSON branches of both (which carry structured data) ARE in scope.

## D-004: No bug-for-bug parity with `serve`

Date: 2026-05-07
Affected requirements: (all)
Status: rejected
Reason: Anti-hallucination rule #5. Where `serve` exhibits a behavior that looks unintentional (e.g. the JSON listing leaking absolute filesystem paths in its `dir` field — see Q-008; or the multi-slash-collapse only firing when `trailingSlash` is set — see Q-006), IrServe is free to diverge with a documented adaptation.
Impact: Future SRVs that catch a quirk should record an `adapted` status here when IrServe chooses not to mirror it. This is a meta-decision; it does not by itself rule out matching `serve`.

## D-005: Clipboard side effect is not implemented

Date: 2026-05-07
Affected requirements: SRV-CLI-011 (`-n`/`--no-clipboard`)
Status: rejected
Reason: Writing the bound URL to the system clipboard on startup is environment-coupled (display server, OS-specific clipboard daemons), interferes with non-interactive use, and is not part of the HTTP contract.
Impact: IrServe MUST accept the `-n`/`--no-clipboard` flag without error so existing scripts keep working, but it has no observable effect because IrServe never touches the clipboard.

## D-006: HTTP compression is L3-priority, not MVP-mandatory

Date: 2026-05-07
Affected requirements: SRV-CLI-012 (`-u`/`--no-compression`)
Status: adapted
Reason: `serve` uses the `compression` connect/express middleware with default settings. Implementing equivalent behavior in Rust adds dependencies (`flate2`, content-type sniffing, threshold logic) that are not justified for an L0–L2 MVP. The `--no-compression` flag stays in the CLI.
Impact: Until L3 work begins, IrServe MAY ignore `--no-compression` and serve responses uncompressed regardless. The `Vary: Accept-Encoding` header MAY still be omitted in this early phase; oracle tests for compression are deferred until the implementation lands.
