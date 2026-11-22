# Spec Delta

## Purpose

Lets blocks be moved, reordered, and folded without losing their subtree or their place in the graph.

## ADDED Requirements

### Requirement: A block moves with its subtree
A block SHALL move, together with all of its descendants, to a new position among siblings or to another page. A move into the block's own subtree SHALL be refused. A move SHALL be one undoable action.

#### Scenario: Reorder among siblings
- **WHEN** a block is moved above or below a sibling
- **THEN** it and its descendants take the new place, and the other siblings keep their order

#### Scenario: Move across pages
- **WHEN** a block is moved to another page
- **THEN** the whole subtree leaves the old page and appears on the new one

#### Scenario: Move into itself
- **WHEN** a block is moved inside one of its own descendants
- **THEN** the move is refused and nothing changes

#### Scenario: One undo
- **WHEN** a move succeeds and the user presses undo once
- **THEN** the block and its subtree return to their old page, parent, and position

### Requirement: Blocks reorder from the keyboard
With the caret in a block, Alt+Up SHALL swap it with its previous sibling and Alt+Down with its next sibling. At the first or last sibling the key SHALL do nothing. The caret SHALL stay in the moved block.

#### Scenario: Move up and down
- **WHEN** the user presses Alt+Up or Alt+Down on a block that has a sibling in that direction
- **THEN** the block swaps with that sibling and keeps the focus

#### Scenario: No sibling
- **WHEN** the block is the first or last sibling
- **THEN** the key does nothing

### Requirement: Blocks can be dragged and sent to another page
A block SHALL offer a drag affordance that reorders it within the visible page and a "move to page" action that sends it, with its subtree, to the end of a chosen page or journal day.

#### Scenario: Drag to reorder
- **WHEN** a block is dragged above or below another block on the page
- **THEN** it lands in that position and one undo restores the old one

#### Scenario: Move to page
- **WHEN** the user chooses another page for a block
- **THEN** the block and its subtree appear at the end of that page

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
