# Tasks and agenda

A block can carry a TODO or DONE marker, a scheduled date, or both. Scheduled work appears in an
agenda section under today's blocks, and completing a task removes it from the agenda.

## Sub-features

- `todo-cycle` marks the focused block TODO with Ctrl+Enter.
- `schedule-calendar` opens the calendar with Ctrl+Shift+S or `/sch`.
- `schedule-set` writes the chosen day as the block's scheduled date.
- `agenda-list` lists today's scheduled work under today's blocks.
- `agenda-open` clicks an agenda row.
- `agenda-done` marks the block DONE from the agenda and removes the row.
- `slash-schedule` schedules through the slash menu and undoes it in one step.

## How to get to it (user POV)

- Put the caret in a block and press Ctrl+Enter.
- Press Ctrl+Shift+S, or type `/sch` in a block, to open the calendar. Arrow keys move the day.
- Read the agenda section under today's blocks in the journal.
- Click the checkbox on an agenda row to complete it.
- Choose the scheduled command in the slash menu, then accept the calendar.

## Driving it with livetest

Preconditions:

- `scripts/doctor.sh` passes.
- `scripts/verify.sh` has produced `livetest.log` and `shots/`.
- The journal steps ran first, so the first block holds text.

- **Mark TODO.** Read `PASS ctrl+enter marks the block TODO: markers=[Some(Todo), None]`. Shot
  `10-todo`.
- **Schedule from the calendar.** Read `PASS picking today scheduled the block` and
  `PASS the agenda section appeared under today's blocks`. Shots `11-date-picker`, `12-agenda`.
- **Complete from the agenda.** Read `PASS the agenda row can be clicked`,
  `PASS checking it off marks the block DONE`, and `PASS the completed row left the agenda`. Shot
  `13-agenda-done`.
- **Schedule from the slash menu.** Read
  `PASS the scheduled command opens the calendar, and accepting it dates the block` and
  `PASS one undo takes back the command's date and the token it replaced`. Shot `34-slash-scheduled`.
- **Proof.** Keep shot `12-agenda` beside the log. The label detail names the scheduled date, so
  quote that value.

## Gotchas

- The agenda only shows work scheduled for today. A date picked tomorrow appears on that day's page,
  not in today's agenda.
- The scheduled date is relative to the run date. Labelled dates change from day to day.
- The undo after `/sch` removes both the date and the token it replaced. One step, not two.
- Completing a task changes the marker to DONE. It does not delete the block.
- The calendar is app-owned and openable with `open_named` if the shortcut is under test. Do not
  mistake the native file dialogs for it.
