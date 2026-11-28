# Proposal

## Why

LogNotes already holds the daily record of a project — the journal entries that link a project page — but the
project page does not consolidate that record. Reading a year of journal blocks by hand to answer “where does
this project stand?” is exactly the work an agent can do well.

The app has no way for an agent to read the graph or write a bounded result back. Exporting Markdown gives a
snapshot, not a live, structured view, and direct database access would bypass link indexing, undo, and the
single-writer model. What is missing is a small, local agent bridge: read access through the app, and a narrow
write surface for a project digest and the tasks it produces.

## What Changes

- Add a local **agent bridge**: an MCP server that talks to the running LogNotes app over an authenticated
  loopback control channel. The app remains the only writer.
- Add **project context**: for a named project page, return the current digest, human notes, and every journal
  source root that explicitly links the project, with the root’s children and ancestors.
- Add an **agent-owned project digest section** on the project page:
  `## Agent digest (auto-generated)` with current state, decisions, open questions, a timeline, and a visible
  `Last consolidated` date. The agent replaces only that subtree.
- Add **agent-created tasks** in today’s journal, grouped under a marked root such as
  `[agent] [[Project Alpha]] tasks`. The agent creates missing tasks, deduplicates them, and marks its own tasks
  `DONE` when the source says the work is finished; it never rewrites a task’s text.
- Exclude `[agent]` task subtrees from consolidation sources so the agent cannot feed on its own output.
- Make one consolidation a single undoable action across the project page and today’s journal.
- Ship the MCP binary and document how to configure it in an agent client; keep all access local and
  authenticated.

Out of scope for this change:

- generic “edit any block” or “run SQL” tools for agents;
- an in-app LLM chat, summarizer, or provider configuration;
- embeddings or semantic search;
- plain-mention consolidation (explicit links only in v1);
- automatic task scheduling or task-text rewriting;
- remote access, syncing, or a hosted service;
- changing the Markdown export/import contract.

## Capabilities

### New Capabilities

- `agent-bridge`: the local MCP-to-app bridge, its discovery and authentication, the writer boundary, and the
  tool surface that lets an agent read and update LogNotes safely while the app is running.
- `project-digest`: project source context and the agent-owned digest section on a project page, including the
  timeline, the consolidation write, and its undo semantics.
- `agent-tasks`: agent-created tasks in today’s journal, their marked block, deduplication, completion, and the
  rule that keeps them out of future consolidation context.

### Modified Capabilities

<!-- None: existing page, block, journal, and storage behavior is unchanged. -->

## Impact

- `lognotes-core`: a project-context query over journal references and subtrees; a digest-and-task write that
  composes into one undo step; no schema migration in v1.
- `lognotes-qt`: a loopback control server, a per-launch token/control file, new worker jobs with replies, and
  snapshot refresh after an agent write.
- New `lognotes-mcp` binary/crate: MCP over stdio, tool schemas, and a client for the local control channel.
- Packaging and documentation: the MCP binary must ship with the app; an agent-client configuration example is
  needed.
- Tests: context selection, digest replacement, timeline deduplication, task creation/closure, the `[agent]`
  exclusion, authentication, app-not-running errors, and one-undo reversibility.
- The app must be running for agent writes. The control channel is loopback-only and token-authenticated; it is
  not a remote API.
