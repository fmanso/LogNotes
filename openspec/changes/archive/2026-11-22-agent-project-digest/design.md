# Design

## Context

See `proposal.md` for motivation and the three delta specs for the behavior contract.

The current shape is:

```text
QML -> Journal invokables -> channel -> worker thread -> Store -> SQLite
                                      |
                                      +--> full snapshot back to the model
```

- `lognotes-core` owns the `Store`: pages, blocks, links, FTS, agenda, undo, export/import. `references`,
  `page_blocks`, `insert_block`, `update_content`, and `set_marker` are already public.
- `lognotes-qt` owns the one worker thread and the `Store`. The worker loop receives `Job`s. `Flush` already
  demonstrates a command with a completion channel.
- `lock_data_folder` takes an exclusive `<database>.lock` before the store opens. The app is the only writer.
- The database path is resolved in `lognotes-qt/src/main.rs`; the worker is given the path before the window
  loads.
- The app has no HTTP server, no MCP binary, and no external write path.
- The database is canonical. Direct SQL writes would bypass `reindex`, undo, and the snapshot model.

The journal convention this design assumes is one explicit project root with children:

```text
2026-11-22
  - Worked on [[Project Alpha]]
      - decided the cache belongs in core
      - open question: invalidation
```

## Goals / Non-Goals

**Goals:**

- A local agent can read project context and write one consolidation while LogNotes is running.
- The app remains the only writer; links, undo, and snapshots behave as they do for human edits.
- The digest and task contracts are enforced by code, not only by prompt text.
- The agent surface is small enough to audit: named tools, no SQL, no generic block editing.
- No schema migration in v1.

**Non-Goals:**

- An in-app LLM chat or provider configuration.
- Remote access, sync, or a hosted service.
- Embeddings or semantic search.
- Generic agent write tools for arbitrary blocks, pages, settings, backups, or restore.
- Plain-mention consolidation in v1.
- Automatic task scheduling or task-text rewriting.
- A second writer outside the app process.

## Decisions

### 1. The app owns the bridge; the MCP binary is a thin adapter

```text
agent
  |  MCP over stdio
  v
lognotes-mcp
  |  loopback HTTP/JSON + bearer token
  v
lognotes-qt control thread
  |  Job with a reply channel
  v
worker thread -> Store -> SQLite
```

The alternative — the MCP server opening the database and writing directly — is rejected. It would bypass
`reindex`, produce no app-visible undo, and leave the app's snapshots stale. A second writer also violates the
data-folder lock invariant.

The alternative — the app speaking MCP itself — is rejected because MCP clients normally launch the server over
stdio. A GUI process is not a good stdio server, and hosting MCP over HTTP reduces client compatibility. A thin
companion binary keeps the app's process model unchanged.

### 2. Control channel: loopback HTTP with a per-launch token

The app starts a small HTTP listener on `127.0.0.1` with an ephemeral port after the store is ready. It writes a
control file beside the database:

```json
{
  "port": 43117,
  "token": "<32 random bytes, hex>",
  "pid": 12345
}
```

- File permissions are user-only (0600 on Unix; the user profile on Windows).
- The listener binds loopback only.
- Requests carry `Authorization: Bearer <token>`.
- The file is written after readiness and removed on a clean exit; a stale file is harmless because the token is
  per launch and the port will refuse.
- The endpoint is one `POST /rpc` method with a JSON body, plus `GET /health`. A single endpoint keeps the
  surface small and makes `curl` useful during development.
- Request bodies are capped; malformed JSON, wrong methods, bad tokens, and worker timeouts get explicit errors.
- The token never appears in logs.

**Alternative considered:** a Unix domain socket on Linux and a named pipe on Windows. It avoids a listening
port, but adds a platform split to a codebase that currently keeps Windows and Linux on one tree. Loopback TCP
is the smaller, cross-platform choice; the cost is a possible one-time firewall prompt on Windows.

**Alternative considered:** a fixed port. Rejected because another process can already own it. The port is
ephemeral and published in the control file.

### 3. The MCP server reads discovery data, not the notes

`lognotes-mcp` is a separate binary in the workspace. It:

1. resolves the LogNotes data folder (same platform rules as the app, with a `--data-dir` override for tests);
2. reads the control file;
3. speaks MCP over stdio;
4. forwards each tool call to the running app over loopback.

It never opens the database. If the control file is missing or the connection is refused, every tool returns a
clear "LogNotes is not running" error. It re-reads the control file when a request fails, so an app restart with
a new token is picked up without restarting the MCP server.

The v1 tool set is:

```text
Read:
  get_project_context(page_name, since?, full?)
  search_notes(query, limit?)
  get_page(name)
  get_day(date)
  get_agenda()
  get_backlinks(page_name)

Write:
  apply_project_consolidation(page_name, brief, timeline_entries,
                              tasks_to_create, task_updates, through)
```

`apply_project_consolidation` is the only write tool. There is no `update_block`, `delete_page`, or `run_sql`.

**Alternative considered:** exposing generic block tools and letting the prompt enforce the digest boundary.
Rejected: the model would have to remember the contract on every run, and one mistake could rewrite human notes.

**Alternative considered:** only `get_project_context` and no general read tools. Rejected for v1 because the
same bridge is immediately useful for “what did I decide about X?”, and the read tools are low risk.

### 4. Project context is a core query, not MCP-side glue

Put the context builder in `lognotes-core`, so it is testable without Qt and the MCP server stays a translator.

The input is a project page and an optional `since` date. The output is roughly:

```text
ProjectContext {
  project: Page
  digest: { exists, block_id, last_consolidated, human_notes }
  sources: [
    { day, root: BlockRef, ancestors: [BlockRef], children: [BlockRef] }
  ]
  agent_tasks: [TaskRef]
  human_open_tasks: [TaskRef]
  nothing_new: bool
}
```

Selection rules:

- Use explicit `block_link` references to the project page (`kind = link` or `tag`).
- Keep only journal-page sources. Named pages are out in v1.
- Exclude the project page itself, including the digest and human notes.
- Exclude any source root whose text begins with `[agent]`, and its descendants.
- For each root, include the root and its descendant subtree. Include the ancestor chain for framing only.
- If a day has several roots, keep them as separate sources; the agent merges them in the timeline.
- If two roots are on the same ancestor path, do not promote the ancestor to a separate source.
- `since` filters by journal date, not block id. `full` ignores `since`.
- `nothing_new` is true when the newest source day is not later than `last_consolidated`.

`references(page_id)` already returns linked blocks with their page and journal date. The subtree and ancestor
paths come from `page_blocks` on each source day. A dedicated query can do this in one pass and avoid repeating
the work per root.

**Alternative considered:** building the context in the MCP binary from SQLite. Rejected: it duplicates the
source rules, bypasses the app's single-writer model for writes, and makes the rules harder to test.

### 5. The digest section is a visible text contract

The project page gets at most one top-level block whose text is exactly:

```text
## Agent digest (auto-generated)
```

Its descendants are the agent-owned subtree:

```text
## Agent digest (auto-generated)
  Last consolidated: [[2026-11-22]]
  ### Current state
    - ...
  ### Decisions
    - ... [[2026-11-20]]
  ### Open questions
    - ... [[2026-11-20]]
  ### Timeline
    - 2026-11-20: ... [[2026-11-20]]
```

Why a heading rather than hidden state:

- It is visible and understandable without the agent.
- It survives export/import as ordinary blocks.
- No schema migration is needed.
- If it is renamed, the agent can create a new one; that is visible and fixable.

The v1 lookup is exact text at top level. If there are zero, create it at the top of the page. If there are two,
refuse. A later version can store the section block id in app state and fall back to the heading.

The write replaces only the section's descendants. Every block outside the section keeps its text, order, and
nesting. The digest must not link its own page; the write rejects a self-link rather than silently rewriting
the agent's text.

**Alternative considered:** a separate digest page. It is safer, but it splits the project's home from its
summary. The chosen contract keeps one place to read while still protecting human notes.

**Alternative considered:** block metadata marking generated blocks. LogNotes has no block metadata and
storing a hidden flag would add schema and UI complexity. The visible heading is enough for v1.

### 6. The consolidation write is one core command

`apply_project_consolidation` is one command on the worker. It composes:

1. find or create the digest section;
2. replace the brief children, preserving `Last consolidated` and the timeline subtree;
3. merge timeline entries by date (replace an existing date, append a new one);
4. create task roots/tasks in today's journal;
5. mark selected agent tasks `DONE`;
6. update `Last consolidated`.

The core store needs a public method that builds a composite action. `Store::exec` is private and
`Action::Composite` is private; the new method lives in the store and uses the same inverse-recording path as
other commands. One undo then restores the project page and today's journal together.

Validation before any write:

- target exists and is a named page, not a journal day;
- digest section count is 0 or 1;
- `through` is a valid ISO date and not in the future;
- every timeline entry has a date and a source day;
- every task has a source day;
- a task status update targets a block under an `[agent]` root and changes only its marker;
- a proposed digest does not link its own page;
- the payload is within size limits.

Errors are returned before the first mutation. If a write fails, the composite transaction rolls back and the
old digest and `Last consolidated` remain.

The result returned to the agent:

```text
{
  applied: true,
  through: "2026-11-22",
  source_days: ["2026-11-20", "2026-11-21"],
  tasks_created: 2,
  tasks_closed: 1,
  digest_block_id: 1234,
  task_root_block_id: 1250
}
```

`get_project_context` with `nothing_new: true` lets the agent report “nothing new” without calling apply. Calling
apply with no brief, no timeline, and no tasks is a no-op and does not clear the existing digest.

### 7. Agent tasks live in a marked root on today's journal

For each project and each local journal day, there is at most one top-level root:

```text
- [agent] [[Project Alpha]] tasks
    - TODO Review cache invalidation [[2026-11-20]]
    - TODO Decide cache invalidation strategy [[2026-11-20]]
```

Rules:

- The root begins with `[agent]` and carries the project link; task children do not need the project link.
- Tasks are `TODO` with no `SCHEDULED` date.
- Each task links its source journal day.
- If today's page has no row, the first task creates the page and the root.
- If the marked root exists, reuse it. If there are two matching roots, refuse.
- If a consolidation produces no tasks, do not create an empty root.
- Task text is stable; the bridge never rewrites an existing task's text.
- Closing changes only the marker of a block under an `[agent]` root.
- Dedupe compares normalized text, project, and source day across agent task roots on earlier journal days as
  well as today. A `DONE` match is not recreated.

The `[agent]` prefix is both the exclusion rule for project context and the ownership marker for closing
tasks. If the user removes it, the block becomes ordinary journal content and is no longer agent-owned.

**Alternative considered:** a dedicated `Agent inbox` page. It is clean but adds a page the user must remember.
Today's journal keeps tasks in the daily flow, which is the habit this app is built around.

**Alternative considered:** hidden task ids in a state table. More robust for reworded tasks, but it adds schema
and bookkeeping before the simple convention has proven insufficient.

### 8. Readiness, undo, and the editing window

The control server starts only after the worker reports ready; if the store fails to open, there is no bridge.

An agent write is a normal store command and therefore appears in undo. One undo reverses the entire
consolidation. This is the recovery path for a bad synthesis.

The model reset risk is real: every applied snapshot rebuilds the delegates, and the editor holds unsaved text in
QML. For v1, **an apply is refused with a retryable `editor_busy` error while a block editor is active**. The
agent can retry once the user leaves the block. This avoids silently dropping a keystroke. A later version can
queue the write until the debounce has flushed, but that coordination is not needed to prove the feature.

The control server logs every applied consolidation with the project, through date, source day count, and task
counts. It never logs the token.

### 9. Packaging and client setup

The release must ship both binaries:

```text
lognotes-qt
lognotes-mcp
```

The MCP binary is small and has no Qt dependency. The README shows a local MCP client configuration that points
at the binary; no host, account, or network setting is required. The app must be running for tools that need the
live store.

## Request shapes

The control endpoint is a single JSON-RPC-like method call. The MCP server translates tool arguments into these
requests and translates errors back into MCP tool errors.

```text
POST /rpc
{
  "method": "project_context",
  "params": { "page": "Project Alpha", "since": "2026-11-20", "full": false }
}
```

```text
POST /rpc
{
  "method": "project_consolidation",
  "params": {
    "page": "Project Alpha",
    "through": "2026-11-22",
    "brief": {
      "current_state": [ { "text": "...", "sources": ["2026-11-20"] } ],
      "decisions": [ ... ],
      "open_questions": [ ... ]
    },
    "timeline_entries": [
      { "date": "2026-11-22", "text": "..." }
    ],
    "tasks_to_create": [
      { "text": "...", "source_day": "2026-11-22" }
    ],
    "task_updates": [
      { "block_id": 123, "marker": "DONE" }
    ]
  }
}
```

A structured brief, rather than one opaque markdown blob, lets the bridge enforce citations, section names, and
timeline dates. The MCP tool description documents the shape; the bridge validates it.

## Risks / Trade-offs

- **Direct database access from another process** → never offered; the bridge is the only agent path.
- **A stale control file after a crash** → per-launch token and connection failure; the MCP server reports that
  the app is not running. The file is overwritten on the next launch.
- **Windows firewall prompt on first listen** → loopback-only binding and documentation; a platform-specific
  local socket is a fallback if this proves annoying.
- **Model reset while typing** → refuse with `editor_busy` in v1; queue-until-idle later.
- **Heading convention breaks on rename** → exact heading lookup detects zero or multiple sections and refuses
  rather than guessing; block-id state can replace it later.
- **Task duplicates after rewording** → stable text plus exact normalized dedupe; semantic dedupe is the agent's
  job, not the bridge's.
- **Agent writes enter the user's undo history** → intentional recovery path; one undo reverses the whole
  consolidation.
- **MCP binary drift from the app protocol** → both sides share the request schema and an integration test drives
  the real control endpoint.
- **Packaging a second binary** → include both in the Linux tarball and Windows zip, with a version check from the
  MCP binary against the app's control response.
- **Digest content is only as good as the prompt** → the bridge enforces structure and provenance, not truth;
  unsourced claims must be labelled, and the user can undo.

## Migration Plan

No data migration. The digest and task blocks are ordinary blocks. The bridge is additive: without the control
file, the app behaves exactly as it does today. Rolling back means removing the control server and the MCP
binary; existing digest sections and `[agent]` blocks remain readable notes.

Ship in this order:

1. Core project context and consolidation command, with tests.
2. App control server and worker jobs.
3. MCP binary and tool schemas.
4. Packaging, client configuration, and live integration test.

## Open Questions

- Which MCP SDK/crate to use for the stdio server (protocol correctness outweighs binary size here).
- Whether to include plain mentions as a second, flagged source class after the explicit-link version is in use.
