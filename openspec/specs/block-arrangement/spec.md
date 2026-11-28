# block-arrangement Specification

## Purpose

Lets a block hide its children, and remembers that it is folded.

## Requirements

### Requirement: Folding hides a block's children and is remembered
A block with children SHALL be foldable. Folding SHALL hide its descendants from the view and persist the folded state, so reopening the page or restarting the app keeps them hidden. Folding SHALL NOT enter the undo history.

#### Scenario: Fold and unfold
- **WHEN** a block with children is folded
- **THEN** its descendants disappear and a control shows that it is folded; unfolding brings them back

#### Scenario: Fold survives
- **WHEN** a folded block's snapshot is rebuilt, or the app is restarted
- **THEN** the block is still folded

#### Scenario: Fold is not an edit
- **WHEN** a block is folded
- **THEN** the undo history is unchanged

#### Scenario: Folding a focused parent
- **WHEN** the caret is in a child of a block that is folded
- **THEN** the caret moves to the folded block

### Requirement: A folded block shows that it hides something
A block with children SHALL carry whether it is folded and how many direct children it has, so the view can draw a fold control only where folding means something.

#### Scenario: A leaf block
- **WHEN** a block has no children
- **THEN** no fold control is drawn

#### Scenario: A folded block with children
- **WHEN** a block has children and is folded
- **THEN** the control shows the folded state and the children remain out of the row list
