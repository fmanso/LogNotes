# page-lifecycle Specification

## Purpose

Lets a named page be renamed or removed while keeping the links that point at it coherent.

## Requirements

### Requirement: Renaming a page updates every explicit reference
Renaming a named page SHALL update the page's display name and rewrite the `[[links]]` and `#tags` that resolve to it in every block, outside code spans and fences, as one undoable action. A tag reference whose new name cannot be written as a tag SHALL become a `[[link]]`.

#### Scenario: Rename with links and tags
- **WHEN** a page named `MyProject` is renamed to `Project Alpha`
- **THEN** every `[[MyProject]]` becomes `[[Project Alpha]]`, every `#MyProject` becomes `[[Project Alpha]]`, and the page opens under its new name

#### Scenario: Case-only rename
- **WHEN** `my project` is renamed to `My Project`
- **THEN** the display name changes and references that resolved to the page carry the new spelling

#### Scenario: Code is untouched
- **WHEN** a block contains `[[My Project]]` inside inline code or a fence
- **THEN** that text is unchanged after the rename

#### Scenario: One undo
- **WHEN** a rename succeeds and the user presses undo once
- **THEN** the old page name and every rewritten block are restored together

### Requirement: A rename is refused when the name cannot identify the page
A rename SHALL be refused with a message when the new name is empty, is a strict `YYYY-MM-DD` date, contains brackets or a newline, or matches another page's name ignoring case. Journal days SHALL NOT be renamed.

#### Scenario: Duplicate name
- **WHEN** the user renames a page to a name another page already has
- **THEN** the rename is refused and both pages are unchanged

#### Scenario: Date or journal
- **WHEN** the user tries to rename a page to a date, or tries to rename a journal day
- **THEN** the action is refused with a message and nothing changes

#### Scenario: Malformed name
- **WHEN** the new name is empty or contains `[`, `]`, or a newline
- **THEN** the action is refused with a message and nothing changes

### Requirement: Plain mentions are left as text and reported
Renaming SHALL NOT rewrite prose that mentions the old name without a link. The result SHALL say how many plain mentions were left behind.

#### Scenario: A plain mention
- **WHEN** a block says "the MyProject notes" without brackets and the page `MyProject` is renamed
- **THEN** the prose is unchanged and the result reports the leftover mention

### Requirement: Deleting a page removes it and its blocks
Deleting a named page SHALL remove the page row and all of its blocks as one undoable action. Deleting SHALL be refused when any block still links or tags the page, and the refusal SHALL report how many references remain. Plain mentions SHALL NOT block deletion. Journal days SHALL NOT be deleted.

#### Scenario: Delete an unlinked page
- **WHEN** a named page has no explicit references and the user deletes it
- **THEN** the page and its blocks disappear, and one undo restores both

#### Scenario: Delete a linked page
- **WHEN** a block still contains `[[Target]]` and the user tries to delete `Target`
- **THEN** the deletion is refused, the page stays, and the message names the remaining references

#### Scenario: Plain mentions do not block
- **WHEN** the page is mentioned only as unlinked prose
- **THEN** deletion succeeds

#### Scenario: A journal day
- **WHEN** the page in view is a journal day
- **THEN** no delete action is offered

### Requirement: Page actions live on the page they change
A named page's header SHALL offer rename and delete. Rename SHALL ask for the new name and show the result, including leftover mentions. Delete SHALL ask for confirmation and show why it was refused. A successful rename SHALL keep the page in view under its new name; a successful delete SHALL return to the journal. A refusal SHALL leave the page open.

#### Scenario: Header actions
- **WHEN** a named page is in view
- **THEN** rename and delete are available from its header, and a journal day shows neither

#### Scenario: Successful rename
- **WHEN** a rename succeeds
- **THEN** the header shows the new name and the column stays on the page

#### Scenario: Successful delete
- **WHEN** a delete succeeds
- **THEN** the column returns to the journal

#### Scenario: Refusal keeps the notes open
- **WHEN** a rename or delete is refused
- **THEN** the page stays open and the reason is shown without the error pane
