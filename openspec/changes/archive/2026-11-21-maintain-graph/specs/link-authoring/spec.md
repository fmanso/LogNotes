# Spec Delta

## Purpose

Writes `[[page]]` links with completion over the pages that already exist, so making a link stops being a spelling test.

## ADDED Requirements

### Requirement: Typing `[[` opens page completion
When the caret follows an unclosed `[[` outside code, the app SHALL offer the page names whose name matches the text after the brackets, with exact and prefix matches first.

#### Scenario: Start a link
- **WHEN** the user types `[[` in a block
- **THEN** a list of matching pages appears beside the caret

#### Scenario: Filter as you type
- **WHEN** the user continues with `[[my pro`
- **THEN** the list narrows to names containing that text, prefix matches first

#### Scenario: Written days are offered
- **WHEN** a journal day has been written
- **THEN** its date is part of the names the completion searches

#### Scenario: Inside code
- **WHEN** `[[` is typed inside inline code or a fenced block
- **THEN** no completion appears

### Requirement: Committing inserts a complete link
Enter or a click SHALL replace the typed `[[` and query with `[[Name]]`, place the caret after the closing brackets, and leave the stored text to the normal debounced save. With no matching page, a non-empty query SHALL still close as `[[query]]`, which creates the page on save. Esc SHALL leave the typed text unchanged.

#### Scenario: Choose a page
- **WHEN** the user picks `My Project` from the list
- **THEN** the block contains `[[My Project]]` with the caret after it

#### Scenario: A new name
- **WHEN** the query matches no page and the user presses Enter
- **THEN** the block contains `[[query]]` and saving creates that page

#### Scenario: Esc
- **WHEN** the user presses Esc
- **THEN** the menu closes and the text stays exactly as typed

#### Scenario: Empty query
- **WHEN** the list is open with an empty query and Enter is pressed
- **THEN** the menu closes without inserting brackets or text

### Requirement: Completion is keyboard and mouse driven
Up and Down SHALL move the highlighted page, Enter SHALL commit it, Esc SHALL dismiss the menu, and a click SHALL commit the clicked page. The menu SHALL NOT take focus from the editor, so typing keeps working.

#### Scenario: Keyboard
- **WHEN** the menu is open and the user presses Down then Enter
- **THEN** the second page is inserted

#### Scenario: Mouse
- **WHEN** the user clicks a page in the menu
- **THEN** that page is inserted and the caret returns to the block

### Requirement: Completion follows the current graph
The names offered SHALL come from the same snapshot as the blocks, so a page created, renamed, or deleted since the last edit is reflected without reopening the app.

#### Scenario: New page appears
- **WHEN** a page is created by typing a link and a later block starts `[[`
- **THEN** the new page is in the completion list
