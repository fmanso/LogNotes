# agent-bridge Specification

## Purpose

Lets a local AI agent read and update a running LogNotes graph through a narrow, authenticated bridge, without
becoming a second writer or reaching the database directly.

## Requirements

### Requirement: The bridge exists only while LogNotes is running
The app SHALL expose its agent bridge only after the store is ready and SHALL withdraw it when the app exits.
Requests SHALL be accepted only from the same user over loopback and SHALL require a per-launch credential.

#### Scenario: App ready
- **WHEN** LogNotes has opened the store and an agent connects with the current credential
- **THEN** the agent can call the bridge's tools

#### Scenario: App not running
- **WHEN** an agent tries to connect while LogNotes is not running
- **THEN** it receives a clear "LogNotes is not running" failure and no operation is attempted

#### Scenario: Unauthenticated or remote request
- **WHEN** a request has a missing or stale credential, or does not originate from the local machine
- **THEN** it is refused

### Requirement: The bridge exposes only named agent operations
The bridge SHALL expose only the documented read and write operations for agent use. It SHALL NOT expose SQL,
generic block editing, page deletion, settings changes, backup or restore, or arbitrary file access.

#### Scenario: Documented operation
- **WHEN** an agent asks for project context
- **THEN** it receives the context response and nothing outside the documented shape

#### Scenario: Undocumented operation
- **WHEN** an agent asks to run SQL, delete a page, or edit an arbitrary block
- **THEN** the bridge refuses the request and the notes are unchanged

### Requirement: The app remains the only writer
Every agent write SHALL pass through the app's normal store path so links are reindexed and the change is one
undoable action. The bridge SHALL NOT write the database directly.

#### Scenario: A consolidation writes links
- **WHEN** an agent writes a digest containing a journal day link
- **THEN** that day's references include the new digest block, as they would for a human edit

#### Scenario: Direct database access is absent
- **WHEN** an agent uses the bridge
- **THEN** no database handle or SQL surface is available to it

### Requirement: An agent write is one undoable action
A successful consolidation SHALL enter the app's undo history as one action, covering every block it created,
replaced, or marked. One undo SHALL restore the project page and today's journal together.

#### Scenario: Undo a consolidation
- **WHEN** an agent consolidates a project and the user presses undo once
- **THEN** the digest, timeline, task block, and task markers return to their previous state

#### Scenario: Failed write
- **WHEN** any part of the write fails
- **THEN** no part of the consolidation is applied

### Requirement: Bridge use is local and auditable
The app SHALL keep the bridge's credential readable only by the current user and SHALL log each applied
consolidation with the project, the through date, and the number of tasks created or closed. Logs SHALL NOT
contain the credential.

#### Scenario: Applied consolidation is logged
- **WHEN** an agent consolidates a project successfully
- **THEN** the log names the project, the through date, and the task counts

#### Scenario: App exits
- **WHEN** LogNotes closes
- **THEN** the previous credential no longer grants access

### Requirement: Bridge failures are explicit and harmless
The bridge SHALL reject malformed, oversized, timed-out, or unauthorized requests with an error that names the
problem, and SHALL leave the notes unchanged.

#### Scenario: Malformed request
- **WHEN** an agent sends an invalid payload
- **THEN** it receives an error and nothing is written

#### Scenario: App closes during a request
- **WHEN** LogNotes exits before a request completes
- **THEN** the agent receives a failure and no partial write remains

### Requirement: The agent client is configured with the local server
The release SHALL include the agent bridge executable and documentation showing how a local MCP client starts
it. The configuration SHALL NOT require a network endpoint beyond loopback.

#### Scenario: Agent client setup
- **WHEN** the user follows the documented configuration in a supported local MCP client
- **THEN** the client starts the bridge and lists the documented tools

#### Scenario: No remote endpoint
- **WHEN** the user inspects the documented configuration
- **THEN** it names only a local executable and no remote host or account
