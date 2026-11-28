# Agent bridge

While the app runs, it serves a loopback control channel for a local MCP adapter. An agent can read
project context, then write one digest and its tasks. The write is refused while a block editor owns
the caret, and one undo takes the whole consolidation back.

## Sub-features

- `bridge-ready` starts the bridge after the store is ready and writes a control file beside the
  database.
- `bridge-context` returns the project, its digest, human notes, and the journal sources that link
  it.
- `bridge-busy` refuses a write while a block editor is focused, with `editor_busy`.
- `bridge-apply` writes the digest subtree and creates or closes tasks under an `[agent]` root.
- `bridge-read` reads the digest, timeline, and marked task back through the same surface.
- `bridge-undo` removes the digest and the agent task in one undo.

## How to get to it (user POV)

- The bridge has no visible control. It exists while the app runs.
- The control file `lognotes-agent.json` appears beside the database. It holds the loopback port and
  a per-launch token.
- A configured MCP client calls `get_project_context` and `apply_project_consolidation`.
- The digest appears on the project page under `## Agent digest (auto-generated)`. Tasks appear in
  today's journal under an `[agent] ... tasks` root.

## Driving it with livetest

Preconditions:

- `scripts/doctor.sh` passes.
- `scripts/verify.sh` has produced `livetest.log` and `shots/`.
- The harness reads the control file from its temp root and posts to `127.0.0.1` with the token.

- **Context.** Read `PASS the agent bridge returns project context with journal sources`. The detail
  names the project and at least one source root with its day and ancestors. Shot `31g-agent-project`.
- **Busy refusal.** Read
  `PASS the agent bridge refuses a write while the editor is busy: answer=Err("editor_busy")`.
- **Apply.** Read `PASS the agent bridge applies a digest and a task`. The detail reports
  `applied:true`, a `digest_block_id`, a `task_root_block_id`, and `tasks_created:1`.
- **Read back.** Read
  `PASS the digest, timeline, and marked task are readable through the bridge`. The detail must name
  `Last consolidated`, the four digest headings, and `agent_owned:true` on the task. Shot
  `31h-agent-digest`.
- **Undo.** Read `PASS one undo removes the digest and the agent task`. The context must return to
  `digest.exists:false` and an empty `agent_tasks`. Shot `31i-agent-undone`.
- **Proof.** Keep shot `31h-agent-digest` and the apply and undo labels. The pair proves the write
  and its single-step reversal.

## Gotchas

- The bridge starts only after the store reports ready. A call before that gets
  `LogNotes is not ready`.
- `editor_busy` is correct behavior, not a failure. Click outside the block and retry.
- The bridge is local only and token-authenticated. The token is never logged, and
  `lognotes-agent.json` is user-only.
- The harness uses the temp graph. Never point an agent or the MCP adapter at the real database.
- Plain mentions are not sources. Only explicit links count, and `[agent]` task subtrees are
  excluded so the agent cannot feed on its own output.
- The digest heading is part of the contract. A renamed heading means the write will not replace the
  old subtree.
