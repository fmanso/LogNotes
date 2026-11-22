# Spec Delta

## Purpose

Reaches any calendar day by date, written or not, without writing a database row until something is typed.

## ADDED Requirements

### Requirement: A date can be opened from the journal
The journal SHALL offer a date picker that opens the chosen day as a page, including a past day nobody wrote and a future day.

#### Scenario: Pick a day
- **WHEN** the user picks 2025-11-02 from the journal's date control
- **THEN** that day opens as a page in the column

#### Scenario: Today and the future
- **WHEN** the user picks today or a future date
- **THEN** the same day page opens

### Requirement: An unwritten day opens without creating a row
Opening a day that was never written SHALL show its page and an empty block, and SHALL NOT create a journal page row. The row SHALL be created only when a block is written.

#### Scenario: An empty past day
- **WHEN** the user opens a day with no blocks and types nothing
- **THEN** the database still has no page for that date

#### Scenario: First keystroke
- **WHEN** the user types in the empty block of such a day
- **THEN** the day's page row and the block are created together

### Requirement: A written day opens as its page
A written day opened by the picker SHALL show its blocks and its linked references, exactly as when opened from a date link. It SHALL NOT show the agenda.

#### Scenario: Written day
- **WHEN** the user opens a day that has blocks
- **THEN** the blocks and references are shown, and no agenda section appears

#### Scenario: Date link
- **WHEN** a `[[2025-11-02]]` link is followed
- **THEN** it opens the same day page, with the same rules

### Requirement: The journal is one step away
From a day page, the Journal control SHALL return to the journal column, keeping the column's current place.

#### Scenario: Back to the journal
- **WHEN** the user opens a day and then presses Journal
- **THEN** the journal column returns with the same days loaded
