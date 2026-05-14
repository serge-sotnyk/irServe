# Delta for cli

## MODIFIED Requirements

### Requirement: Default port and host

The server SHALL listen on TCP port 3000 when no `--listen`/`-l` flag
is given and `PORT` is unset. When `PORT` is set in the environment
and no `--listen` is given, the server SHALL listen on that port
instead.

Evidence: SRV-CLI-001 (status: accepted, level: L0); oracle: no probe.

Implementation: First wired by change `001-port-minimal-static-server`
(Stage 5a design; Stage 5b code). The default-port logic lives in
`crates/irserve` (the bin) because clap cannot read environment
variables at compile time — see `design.md` § 5.

Note: Source-level evidence in `serve/source/main.ts` documents the
default. Every existing probe passes an explicit `--listen <port>`, so
the default-port code path is not exercised. This SRV remains
`accepted`; promotion to `verified` requires a Stage-5b probe that
omits `--listen` (logged as `tasks.md` § 6.1 of change 001 and as a
planned `tools/probe/cases/default-port.json` case).

#### Scenario: No flag, no environment variable

- GIVEN no `--listen` flag and unset `PORT`
- WHEN the server starts
- THEN it listens on port 3000

#### Scenario: PORT environment variable

- GIVEN `PORT=4567` in the environment and no `--listen`
- WHEN the server starts
- THEN it listens on port 4567
