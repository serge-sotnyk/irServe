# Delta for cli

## ADDED Requirements

### Requirement: Default port and host

The server SHALL listen on TCP port 3000 when no `--listen`/`-l` flag
is given and `PORT` is unset. When `PORT` is set in the environment
and no `--listen` is given, the server SHALL listen on that port
instead.

Evidence: SRV-CLI-001 (status: accepted, level: L0); oracle: no probe.

Note: Source-level evidence in `serve/source/main.ts` documents the
default. Every existing probe passes an explicit `--listen <port>`, so
the default-port code path is not exercised. This SRV remains
`accepted`; promotion to `verified` is a Stage-5b probe TODO.

#### Scenario: No flag, no environment variable

- GIVEN no `--listen` flag and unset `PORT`
- WHEN the server starts
- THEN it listens on port 3000

#### Scenario: PORT environment variable

- GIVEN `PORT=4567` in the environment and no `--listen`
- WHEN the server starts
- THEN it listens on port 4567

### Requirement: Numeric port for `-l`/`--listen`

The `-l`/`--listen` flag SHALL accept a numeric port. The server SHALL
then listen on TCP port _N_ on the default interface. Multiple `-l`
flags SHALL be additive (per the help text "more than one may be
specified").

Evidence: SRV-CLI-002 (status: verified, level: L0); oracle: ORC-001.

#### Scenario: Bare port number

- GIVEN `serve -l 3010` started in a directory
- WHEN a client opens `http://127.0.0.1:3010/`
- THEN it reaches the server

### Requirement: `tcp://host:port` URI for `-l`/`--listen`

The `-l`/`--listen` flag SHALL accept a `tcp://host:port` URI. The
server SHALL bind the given TCP port on the given host.

Evidence: SRV-CLI-003 (status: accepted, level: L1); oracle: no probe (transitive over SRV-CLI-002).

Note: Treated as transitive over the bare-port form (SRV-CLI-002). No
HTTP-level divergence is expected vs. the numeric form. Q-001 tracks
the open question of whether the URI defaults port to 3000 / host to
`localhost` when either is missing.

#### Scenario: TCP URI

- GIVEN `serve -l tcp://127.0.0.1:3011`
- WHEN a client opens `http://127.0.0.1:3011/`
- THEN it reaches the server

### Requirement: `-p` deprecated alias for `-l`/`--listen`

The `-p` flag SHALL be accepted as an alias for `-l`/`--listen` and
SHALL parse the same value forms.

Evidence: SRV-CLI-006 (status: accepted, level: L1); oracle: no probe (covered transitively by every `-l` probe).

Note: `-p` is documented in source comments as kept for backwards
compatibility. IrServe accepts it without further documentation.

#### Scenario: Deprecated alias

- GIVEN `serve -p 3010`
- WHEN the server starts
- THEN it behaves identically to `serve -l 3010`

### Requirement: Directory positional argument

A single optional positional argument SHALL select the directory to
serve. If omitted, the current working directory SHALL be served.
Supplying more than one positional argument SHALL be a fatal error
(non-zero exit code).

Evidence: SRV-CLI-007 (status: verified, level: L0); oracle: ORC-001 (scenarios 1–2), ORC-062 (scenario 3).

#### Scenario: No positional

- GIVEN `serve` invoked in directory `D`
- WHEN no positional argument is given
- THEN files served are resolved relative to `D`

#### Scenario: Single positional

- GIVEN `serve ./public`
- WHEN started
- THEN files served are resolved relative to `<cwd>/public`

#### Scenario: Two positionals is a fatal error

- GIVEN `serve a b`
- WHEN started
- THEN the process exits with a non-zero code and an error message on stderr

### Requirement: `-s`/`--single` SPA fallback

The `-s`/`--single` flag SHALL serve `/index.html` (status 200, the
file's MIME type) for any request whose path does not resolve to a
file. The fallback SHALL be implemented as a high-priority rewrite
and SHALL therefore be overridden by an earlier-matching redirect
(per the operation precedence in the routing capability).

Evidence: SRV-CLI-008 (status: verified, level: L2); oracle: ORC-029.

#### Scenario: SPA fallback for missing route

- GIVEN `serve --single` over a directory containing `index.html`
- WHEN `GET /no/such/route`
- THEN status is 200 and body is the contents of `index.html`

#### Scenario: Redirect beats SPA fallback

- GIVEN `serve --single` and a `serve.json` with a redirect from `/old` to `/new`
- WHEN `GET /old`
- THEN status is 301 (the redirect, not the SPA fallback)

### Requirement: `-c`/`--config` selects a custom configuration file

With `-c <path>`/`--config <path>`, the named file SHALL be read first
as the configuration source. If the file cannot be read, startup SHALL
fail with a non-zero exit code (in contrast to the implicit `serve.json`
lookup, where missing files are silently skipped).

Evidence: SRV-CLI-009 (status: verified, level: L1); oracle: ORC-006.

#### Scenario: Missing config file is fatal

- GIVEN `serve -c ./missing.json`
- WHEN started
- THEN the process exits with a non-zero code

#### Scenario: Custom config file is loaded

- GIVEN `serve -c ./conf.json` and the file contains `{ "cleanUrls": false }`
- WHEN started
- THEN `cleanUrls` is disabled

### Requirement: `-C`/`--cors` enables permissive CORS headers

With `-C`/`--cors`, every response SHALL carry
`Access-Control-Allow-Origin: *`. The flag SHALL also enable additional
permissive CORS response headers; the full response-header surface is
documented as a separate L3 requirement under the `cors` capability and
is not part of this baseline.

Evidence: SRV-CLI-010 (status: verified, level: L1); oracle: ORC-054, ORC-055.

#### Scenario: ACAO header on every response

- GIVEN `serve --cors`
- WHEN any request is made
- THEN the response includes `Access-Control-Allow-Origin: *`

### Requirement: `-n`/`--no-clipboard` is accepted as a no-op

The `-n`/`--no-clipboard` flag SHALL be accepted (silently or with no
observable effect) so that scripts which pass it continue to work.
IrServe SHALL NOT interact with the clipboard at any time, regardless
of the flag.

Evidence: SRV-CLI-011 (status: accepted, level: L1); oracle: no probe (per D-005, clipboard interaction is unobservable via HTTP).

Note: Per `decisions.md` D-005, IrServe does not modify the clipboard.
The flag is accepted for CLI compatibility and ignored.

#### Scenario: Flag is accepted

- GIVEN `serve --no-clipboard -l 3010 ./fixture`
- WHEN started
- THEN startup succeeds and the server is reachable
- AND the system clipboard is not modified

### Requirement: `-d`/`--debug` toggles verbose output

The `-d`/`--debug` flag SHALL be accepted. IrServe MAY map it to a
verbosity level of its own choosing. The exact log format is
implementation-defined.

Evidence: SRV-CLI-014 (status: adapted, level: L1); oracle: no probe (debug verbosity is terminal output, excluded by D-002).

Note: Per `decisions.md` D-002, exact terminal output / stdout
formatting is not in scope. Oracle tests MUST NOT compare stdout/stderr
text for this flag.

#### Scenario: Flag is accepted

- GIVEN `serve --debug -l 3010 ./fixture`
- WHEN started
- THEN startup succeeds and the HTTP behavior is unchanged from the no-debug case

### Requirement: `-L`/`--no-request-logging` silences per-request logs

The `-L`/`--no-request-logging` flag SHALL be accepted. When set,
IrServe SHALL NOT emit per-request log lines to stdout or stderr. The
exact log format when the flag is unset is implementation-defined.

Evidence: SRV-CLI-015 (status: adapted, level: L1); oracle: no probe (logging surface excluded by D-002).

Note: Per `decisions.md` D-002, exact terminal output is not in scope.
IrServe MAY default to silence and treat this flag as a no-op.

#### Scenario: Flag is accepted, no per-request logs emitted

- GIVEN `serve --no-request-logging -l 3010 ./fixture`
- WHEN HTTP requests are processed
- THEN no per-request log line is written to stdout or stderr

### Requirement: `--no-port-switching` disables fallback to a random port

By default, if the requested port is already taken, the server SHALL
pick a free port. With `--no-port-switching`, the server SHALL fail to
start (non-zero exit code) instead of falling back.

Evidence: SRV-CLI-016 (status: accepted, level: L1); oracle: no probe (every probe runs against a free port; failure-mode is a Stage-5b TODO).

Note: Every existing probe passes the flag with an already-free port,
so only the happy path is exercised. The contract — refuse to fall back
when the port is occupied — needs a probe that occupies the port first.

#### Scenario: Refuse fallback on occupied port

- GIVEN port 3000 is occupied
- WHEN `serve --no-port-switching -l 3000` is started
- THEN startup fails with a non-zero exit code

### Requirement: `--help` and `-v`/`--version` exit cleanly

`--help` SHALL print help text and exit 0. `--version`/`-v` SHALL print
the version and exit 0. Each SHALL take precedence over starting the
server.

Evidence: SRV-CLI-019 (status: verified, level: L0); oracle: ORC-060, ORC-061.

Note: Per `decisions.md` D-002, the exact help-text formatting and the
exact version string are out of scope. The version SHOULD be IrServe's
own version in `MAJOR.MINOR.PATCH` form.

#### Scenario: `--help` exits 0

- GIVEN `serve --help`
- WHEN executed
- THEN exit code is 0
- AND stdout is non-empty
- AND stderr is empty
- AND the server is not started

#### Scenario: `--version` exits 0

- GIVEN `serve --version`
- WHEN executed
- THEN exit code is 0
- AND stdout contains a version string
- AND stderr is empty
- AND the server is not started
