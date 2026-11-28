# project-digest Specification

## Purpose

Makes a named project page a consolidated, dated brief built from the journal entries that explicitly link it,
while keeping human notes and the daily record intact.

## Requirements

### Requirement: Project context comes from explicit journal references
For a named project page, context SHALL include each journal block that explicitly links the project, the
block's descendants, and its ancestor framing. The project page itself and any `[agent]` task subtree SHALL be
excluded. Plain-text mentions SHALL NOT be treated as sources in this version.

#### Scenario: A root with children
- **WHEN** a journal block says `Worked on [[Project Alpha]]` and has child blocks with details
- **THEN** the context contains the root and all of its descendants

#### Scenario: A nested root
- **WHEN** the linking block sits under a parent block
- **THEN** the context contains the linking block, its descendants, and its ancestors as framing

#### Scenario: The project page
- **WHEN** the project page's own notes or digest mention the project name
- **THEN** they are not returned as journal sources

#### Scenario: Agent tasks
- **WHEN** today's journal contains an `[agent] … tasks` root that links the project
- **THEN** that subtree is excluded from the source set

#### Scenario: Plain mention
- **WHEN** a journal block names the project only as plain text
- **THEN** it is not a source in this version

### Requirement: A project page has one agent-owned digest section
A named project page SHALL hold at most one top-level `## Agent digest (auto-generated)` section. A
consolidation SHALL write only that section's subtree and SHALL leave every block outside it untouched. If the
section is absent, it SHALL be created; if more than one exists, the write SHALL be refused.

#### Scenario: First consolidation
- **WHEN** a project page has no digest section
- **THEN** the section is created and human notes remain as they were

#### Scenario: Later consolidation
- **WHEN** the digest section already exists
- **THEN** only its subtree is replaced and blocks outside it keep their text, order, and nesting

#### Scenario: Ambiguous section
- **WHEN** the page has two digest sections
- **THEN** the write is refused and the page is unchanged

#### Scenario: Self-link
- **WHEN** a proposed digest links the page it lives on
- **THEN** the write is refused or the self-link is removed before writing

### Requirement: The digest separates a living brief from a timeline
The digest SHALL contain current state, decisions, open questions, a visible last-consolidated date, and a
timeline with at most one entry per journal day. Re-consolidating SHALL replace the brief, SHALL preserve
existing timeline entries, and SHALL NOT add a second entry for a day that already has one.

#### Scenario: A new day
- **WHEN** a new journal day contributes to an existing digest
- **THEN** its timeline entry is added and the brief is updated

#### Scenario: Re-running the same day
- **WHEN** the same consolidation is applied again
- **THEN** the timeline still has one entry for that day and the page has no duplicate blocks

#### Scenario: Existing history
- **WHEN** the brief is replaced
- **THEN** timeline entries that were already present remain

### Requirement: Consolidated claims carry source days
Every consolidated claim SHALL cite the journal day that supports it, and any statement with no source SHALL
be marked as unsourced. A timeline entry SHALL link its journal day.

#### Scenario: A decision
- **WHEN** the digest records a decision found on 2026-11-20
- **THEN** the decision text includes `[[2026-11-20]]`

#### Scenario: An unsourced statement
- **WHEN** the agent cannot attribute a statement to a journal day
- **THEN** the statement is labelled as unsourced or omitted

#### Scenario: A timeline entry
- **WHEN** a day is added to the timeline
- **THEN** its entry links that journal day

### Requirement: Consolidation is validated and reports its result
A consolidation SHALL accept a brief, timeline entries, a through date, tasks to create, and task status
changes. It SHALL reject a missing or future through date, an undated timeline entry, a journal day as the
target, or a task without a source day. It SHALL return the source days used and the counts it created or
closed, and SHALL update the last-consolidated date only on success.

#### Scenario: Successful result
- **WHEN** a valid consolidation is applied
- **THEN** the result names the through date, the source days used, and how many tasks were created or closed

#### Scenario: Invalid target
- **WHEN** the target is a journal day or does not exist
- **THEN** the write is refused and no notes change

#### Scenario: Invalid date
- **WHEN** the through date is missing, malformed, or in the future
- **THEN** the write is refused and no notes change

#### Scenario: Failure
- **WHEN** a write fails partway
- **THEN** the previous last-consolidated date and digest remain

### Requirement: Nothing new is not invented work
When no journal source has appeared since the last consolidation, the operation SHALL be able to report that
there is nothing new and SHALL NOT invent activity, change the brief, or add timeline entries.

#### Scenario: No new sources
- **WHEN** the latest source day is not later than the last-consolidated date
- **THEN** the result can say there is nothing new and the digest stays unchanged

#### Scenario: Full re-synthesis
- **WHEN** the caller explicitly asks for a full re-synthesis
- **THEN** all explicit journal sources are read again and the brief is rebuilt

### Requirement: Human notes are context, never a write target
The project page's blocks outside the digest SHALL be available to the agent as context and SHALL remain
untouched by the consolidation write.

#### Scenario: Human notes inform the brief
- **WHEN** the human notes contain a project constraint
- **THEN** the agent can use it in the brief

#### Scenario: Human notes survive
- **WHEN** a consolidation replaces the digest
- **THEN** the human notes outside the section are byte-for-byte unchanged
