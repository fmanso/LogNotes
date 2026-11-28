# Tasks

## 1. Project context in core

- [x] 1.1 Add the project-context types and query in `lognotes-core`: explicit journal links to a named
  page, root plus descendant subtree, ancestor framing, exclusion of the project page and `[agent]` roots,
  `since`/`full` filtering, and `nothing_new`; verify with tests for a root with children, a nested root,
  the project page itself, an agent task root, a plain mention, and the date window.
- [x] 1.2 Add digest-section parsing: exact `## Agent digest (auto-generated)` heading, zero/one/many
  detection, `Last consolidated` date, human notes outside the section, and malformed-date handling; verify
  with tests for missing, one, duplicate, and malformed sections.
- [x] 1.3 Add existing agent-task and human open-task discovery for deduplication; verify tests show an
  agent task found across an earlier journal day and a human task left unowned.
- [x] 1.4 Document the context selection and digest layout in `lognotes-core/SPEC.md`; verify the text
  matches the implemented tests and `specs/project-digest/spec.md`.

## 2. Consolidation write in core

- [x] 2.1 Add one public store command that applies a structured brief, timeline entries, and `Last
  consolidated` as a single composite action; validate named-page target, ISO/future dates, self-links,
  duplicate sections, and no-op input; verify tests for one undo, human notes untouched, timeline merge,
  invalid date, self-link refusal, duplicate-section refusal, and unchanged `PRAGMA user_version`.
- [x] 2.2 Add agent-task creation and closure to the same composite: today's marked root, project link on
  the root, source-day links on tasks, reuse of an existing root, no empty root, no scheduling, dedupe across
  days, and closing only blocks under `[agent]`; verify tests for create, re-run, earlier-day duplicate,
  `DONE` non-recreation, human-task refusal, text-rewrite refusal, and one undo.
- [x] 2.3 Add the consolidation result shape (through date, source days, counts, block ids) and explicit
  errors for invalid or partial writes; verify tests show a failed write leaves the previous digest, tasks,
  and last-consolidated date intact.
- [x] 2.4 Document the agent task and consolidation conventions in `lognotes-core/SPEC.md`; verify the text
  matches `specs/agent-tasks/spec.md` and `specs/project-digest/spec.md`.

## 3. App control bridge

- [x] 3.1 Add the loopback control server with an ephemeral port, per-launch bearer token, user-only control
  file, request-size limit, timeouts, malformed-request errors, and wrong-token refusal; verify unit tests
  for each failure and for a successful health/request exchange.
- [x] 3.2 Add worker jobs with reply channels for context and consolidation, keeping the `Store` on the
  worker thread; verify an integration test against a temporary database returns context and applies a
  digest through the real worker path.
- [x] 3.3 Start the control server only after the store is ready and withdraw it on exit or store failure;
  verify the control file appears after readiness, a connection fails after exit, and a failed open exposes
  no bridge.
- [x] 3.4 Add the editor-busy guard and snapshot refresh behavior: refuse with a retryable error while a
  block editor is active, refresh the affected view after a successful apply, and keep unsaved text; verify
  with a live test that busy is refused, the write succeeds after leaving the editor, and the digest appears.
- [x] 3.5 Add the audit log line for an applied consolidation with project, through date, source-day count,
  and task counts, never the token; verify the log line in a temp log and assert the token is absent.
- [x] 3.6 Document the control channel, token lifecycle, writer boundary, and busy behavior in
  `lognotes-qt/SPEC.md`; verify the text matches the implemented behavior and `specs/agent-bridge/spec.md`.

## 4. MCP server

- [x] 4.1 Create the `lognotes-mcp` binary/crate with stdio MCP, data-folder discovery, control-file
  reading, and a client for the loopback endpoint; verify it lists the documented tools and returns
  "LogNotes is not running" when no control file exists.
- [x] 4.2 Implement the read tools (`get_project_context`, `search_notes`, `get_page`, `get_day`,
  `get_agenda`, `get_backlinks`) and map bridge errors to MCP tool errors; verify a contract test against a
  fake control server checks each request and response.
- [x] 4.3 Implement `apply_project_consolidation` with the structured brief, timeline, task, and status
  schema; verify a contract test against a fake server and a real integration test through the app's control
  endpoint.
- [x] 4.4 Add the agent-client configuration example and the tool/contract description to the docs; verify
  the documented command starts the MCP binary and lists the tools.

## 5. Packaging and design record

- [x] 5.1 Add `lognotes-mcp` to the Linux tarball and Windows zip build paths; verify `make dist` includes
  both binaries and the MCP binary runs from the unpacked artifact.
- [x] 5.2 Update `README.md` and `architecture.md` with the local agent bridge, its single-writer boundary,
  the app-must-be-running rule, and the digest/task conventions; verify the documented commands and file
  layout match the release artifact.

## 6. Integration

- [x] 6.1 Add a live-test project with a project page and journal roots-with-children; call the context
  endpoint and verify the roots, descendants, ancestors, project-page exclusion, and agent-task exclusion.
- [x] 6.2 Add a live-test consolidation through the control endpoint: apply a brief, timeline entry, and
  tasks; verify the digest section, one timeline entry, the `[agent]` task root, task markers, human notes,
  and one undo restoring the project page and journal together; capture screenshots.
- [x] 6.3 Run `make test`, `make livetest`, and `make dist`; verify every test and live step passes, the
  release contains both binaries, and the database `user_version` is unchanged by the new commands.

## Workflow follow-up

- Archive the change after implementation and the live screenshots have been reviewed.
