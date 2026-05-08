# Delta for cli

## MODIFIED Requirements

### Requirement: Default port and host

The server SHALL listen on TCP port 3000 when no `--listen`/`-l` flag
is given and `PORT` is unset. When `PORT` is set in the environment
and no `--listen` is given, the server SHALL listen on that port
instead.

Evidence: SRV-CLI-001 (status: verified, level: L0); oracle: ORC-063
(`cases/default-port-l0.json#root`, env-var scenario).

Implementation: First wired by change `001-port-minimal-static-server`
(Stage 5a design) and landed by change `002-implement-strict-l0-runtime`
(Stage 5b code). The default-port logic lives in `crates/irserve` (the
bin) because clap cannot read environment variables at compile time;
see change 001 `design.md` § 5 and change 002 `design.md` § 1.

Note: The env-var scenario is verified by ORC-063. The no-flag-no-env
scenario (port 3000 hardcoded) remains source-evidence only because
probing it requires port 3000 to be free at run time, which is flaky
on developer machines. Promotion of that scenario to `verified` is
deferred to a future change with a `defaultPortScenario:
"no-flag-no-env"` probe added to `default-port-l0.json` (or a sibling
case).

#### Scenario: No flag, no environment variable

- GIVEN no `--listen` flag and unset `PORT`
- WHEN the server starts
- THEN it listens on port 3000

#### Scenario: PORT environment variable

- GIVEN `PORT=4567` in the environment and no `--listen`
- WHEN the server starts
- THEN it listens on port 4567
