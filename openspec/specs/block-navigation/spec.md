# block-navigation Specification

## Purpose

Moves the caret between blocks from the keyboard, so a day can be edited without reaching for the mouse.

## Requirements

### Requirement: Up and Down cross into the neighbouring block at a text edge
With no selection, pressing Up while the caret is on the first line of a block SHALL move the caret to the end of the previous editable block. Pressing Down on the last line SHALL move it to the start of the next editable block. Within a multi-line block the same keys SHALL stay inside the text.

#### Scenario: Up from the first line
- **WHEN** the caret is on the first line of a block and Up is pressed
- **THEN** the caret moves to the end of the previous editable block

#### Scenario: Down from the last line
- **WHEN** the caret is on the last line of a block and Down is pressed
- **THEN** the caret moves to the start of the next editable block

#### Scenario: Inside a multi-line block
- **WHEN** the caret is on a middle line of a block
- **THEN** Up and Down move within that block's text

#### Scenario: An empty block
- **WHEN** an empty block has the caret
- **THEN** Up and Down move to the previous and next editable blocks

### Requirement: Navigation follows what is on screen
Up and Down SHALL move between the visible editable blocks only, skipping date headings, agenda rows, reference rows, and other read-only rows. At the first or last editable block the key SHALL do nothing.

#### Scenario: Skip a heading
- **WHEN** the previous row is a date heading and Up is pressed on the first block under it
- **THEN** the caret goes to the last editable block above the heading, or stays if there is none

#### Scenario: Skip an agenda row
- **WHEN** Down is pressed at the last block before the agenda section
- **THEN** the caret skips the agenda rows and lands in the next editable block, or stays if there is none

### Requirement: Selection and popups keep their keys
A selection SHALL keep Up and Down for extending the selection. While a completion or command menu is open, Up and Down SHALL belong to that menu.

#### Scenario: Shift selection
- **WHEN** Shift+Up or Shift+Down is pressed
- **THEN** the caret does not leave the block and the text selection changes

#### Scenario: A menu is open
- **WHEN** the slash or link menu is open
- **THEN** Up and Down move the menu's selection, not the caret between blocks
