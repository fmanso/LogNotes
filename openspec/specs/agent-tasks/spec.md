# agent-tasks Specification

## Purpose

Lets an agent turn project journal facts into actionable tasks in today's journal without duplicating work or
feeding its own tasks back into later consolidation.

## Requirements

### Requirement: Agent tasks live in a marked block on today's journal
A consolidation SHALL create at most one top-level task root per project on the current local journal day. The
root SHALL begin with `[agent]` and name the project, and each task SHALL be a child block marked `TODO`. If
today's page does not exist, writing a task SHALL create it. If no task is produced, no empty root SHALL be
created.

#### Scenario: First task of the day
- **WHEN** an agent creates a task for `[[Project Alpha]]` on a day with no agent task root
- **THEN** today's journal gains one `[agent] … [[Project Alpha]] tasks` root with the task as a child

#### Scenario: More tasks later that day
- **WHEN** another consolidation produces a task for the same project on the same day
- **THEN** the existing marked root is reused and no second root is created

#### Scenario: Nothing to do
- **WHEN** a consolidation produces no tasks
- **THEN** no marked task root is created

#### Scenario: Ambiguous roots
- **WHEN** today's page already has more than one matching marked root for the same project
- **THEN** the task write is refused and the page is unchanged

### Requirement: Agent tasks carry provenance
Every task SHALL link the journal day whose source produced it. A proposed task without a source day SHALL be
refused.

#### Scenario: Task from a past day
- **WHEN** the source is a block on 2026-11-20 and the task is created on 2026-11-22
- **THEN** the task appears on today's page and links `[[2026-11-20]]`

#### Scenario: No provenance
- **WHEN** a proposed task has no source day
- **THEN** the write is refused and no task is created

### Requirement: Agent tasks are deduplicated across days
The bridge SHALL prevent an exact duplicate of an existing agent task, comparing normalized text, project, and
source day across earlier journal days as well as today. A task already marked `DONE` SHALL NOT be recreated.

#### Scenario: Re-running a consolidation
- **WHEN** the same source produces a task that already exists
- **THEN** no second task is created

#### Scenario: Completed task
- **WHEN** the matching agent task is already `DONE`
- **THEN** it is not reopened or duplicated

#### Scenario: Earlier day
- **WHEN** the task was created on an earlier journal day for the same source
- **THEN** the duplicate check still finds it

### Requirement: The agent closes only its own tasks
The agent SHALL mark a task `DONE` only when that task is inside a marked `[agent]` task root. It SHALL NOT
change a human task, rewrite a task's text, or delete a task.

#### Scenario: Closing an agent task
- **WHEN** a new source says an agent-created task is finished
- **THEN** that task's marker becomes `DONE` and its text is unchanged

#### Scenario: Human task
- **WHEN** a human task covers the same subject
- **THEN** the agent leaves it untouched and does not claim it

#### Scenario: Text change
- **WHEN** the proposed update would rewrite an existing task's text
- **THEN** the bridge refuses the update

### Requirement: Agent tasks are unscheduled
Agent-created tasks SHALL have no scheduled date. Scheduling SHALL remain a human decision.

#### Scenario: Task creation
- **WHEN** an agent creates a task
- **THEN** the task has no `SCHEDULED` date and does not enter the agenda by itself

#### Scenario: Existing human schedule
- **WHEN** a human later schedules an agent-created task
- **THEN** the schedule is kept and is not cleared by a later consolidation

### Requirement: Agent task subtrees are excluded from consolidation
Any journal root whose text begins with `[agent]` and all of its descendants SHALL be excluded from project
context, so the agent cannot treat its own output as a new source.

#### Scenario: Project-linked task root
- **WHEN** an `[agent] … [[Project Alpha]] tasks` root is present on a journal day
- **THEN** it is not a source for that project's next consolidation

#### Scenario: Human marker removed
- **WHEN** the user removes the `[agent]` marker from a root
- **THEN** that root becomes eligible as a normal journal source

### Requirement: Task changes are one undoable action
All tasks created or closed by one consolidation SHALL enter the undo history as one action, together with the
project digest write.

#### Scenario: Undo tasks
- **WHEN** a consolidation created two tasks and closed one
- **THEN** one undo removes the two and restores the closed task's marker
