# Notch UX + Telegram Message Enhancement Plan

## Summary

Enhance Notch as a minimal desktop utility, not a dashboard. Keep the compact sticky-note feel, but make the end-to-end flow clearer: capture a task, decide now/later, work with a timer, handle alerts, snooze or finish, and recover from mistakes.

Primary UX goal: the user should always understand **what needs attention now**, **what is scheduled next**, and **what action is safe to take**.

Telegram alerts should become event-specific and easier to scan. Scheduled, due-now, timer-finished, and test-connected messages should each have distinct copy with consistent task title, optional note body, timing detail, and action hint.

## Key Changes

### Improve The List View Into A Lightweight Today Surface

- Sort tasks by urgency: fired schedules awaiting action, running timers, upcoming schedules, unscheduled tasks, then done tasks if shown.
- Add compact row badges for `running`, `done`, `scheduled`, `needs start`, and `finished`.
- Replace the plain empty state with two clear actions: create task and open settings.

### Improve Task Detail Flow

- Make primary actions explicit: `Start`, `Pause`, `Reset`, `Done`, `Schedule`.
- Keep the hero timer, but add a small status line showing timer mode/state such as `countdown idle`, `running`, or `timer finished`.
- Add a `Done` action so tasks have closure instead of only delete.
- Add delete confirmation or undo because the current trash button is too easy to trigger.

### Improve Schedule And Alert Handling

- When a schedule fires without auto-start, show an in-app action area with `Start`, `Snooze 5m`, `Snooze 15m`, and `Dismiss`.
- When a countdown finishes, show a clear finished state with `Reset`, `Start again`, and `Done`.
- Keep the existing schedule popover, but rename vague labels: `Repeat` becomes `Schedule type`, `Off` becomes `No schedule`.

### Improve Settings UX

- Group settings by user intent: `Timer`, `Scheduling`, `Alerts`, `Startup`.
- Keep Telegram in its own tab, but add validation feedback near token/chat fields.
- Make global pause visibly reflected in both list and detail views with a small persistent banner, not only a tiny corner label.

## Telegram Templates

Replace the generic `format_message(prefix, title, content, extra)` usage with small event-specific helpers in `telegram.rs`.

### Timer Finished

Use when a countdown reaches zero.

```text
⏰ <b>Timer finished</b>
Task title

Optional task content

<i>Open Notch to reset, restart, or mark done.</i>
```

### Task Due Now

Use when a schedule fires.

For auto-start schedules:

```text
📌 <b>Task due now</b>
Task title

Optional task content

<i>Timer started automatically.</i>
```

For schedules that need manual start:

```text
📌 <b>Task due now</b>
Task title

Optional task content

<i>Open Notch to start or snooze.</i>
```

### Task Scheduled

Use when the user creates or updates a future schedule.

```text
🗓 <b>Task scheduled</b>
Task title

Optional task content

<i>Due: Tue 30 Jun, 14:30</i>
```

### Telegram Test Connected

Use from Settings `Send test`.

```text
✅ <b>Notch connected</b>
Telegram alerts are ready.

<i>You will receive scheduled task and timer alerts here.</i>
```

Keep HTML escaping and `parse_mode=HTML`. Keep messages short and avoid Telegram action buttons for now because the app treats Telegram as best-effort notification, not a remote-control interface.

## Interfaces And Data

Add task lifecycle fields to the shared Rust/TypeScript `Task` model:

- `status: "active" | "done"`
- `completed_at: string | null`

Use serde defaults so old `tasks.json` files load as active tasks.

Add backend commands:

- `complete_task(id)`
- `reopen_task(id)`
- `snooze_task(id, minutes)`
- `dismiss_fired_schedule(id)` if needed to clear the in-app fired state without deleting the task

Update Telegram call sites:

- Schedule set in `commands.rs` uses `format_task_scheduled`.
- Timer completion in `tick.rs` uses `format_timer_done`.
- Schedule fire in `tick.rs` uses `format_schedule_fired`.
- Settings test command uses `format_test_message`.

Add frontend-only list filters:

- Default list hides done tasks.
- A small `Done` toggle shows completed tasks below active tasks.
- No projects, tags, accounts, sync, or large navigation system.

Preserve current architecture:

- Rust remains source of truth for timers and schedules.
- Frontend continues to render state and invoke commands.
- Existing timer/schedule events stay global and routed by task id.
- Telegram sends remain best-effort and non-blocking.

## Test Plan

- Run `npx tsc --noEmit`, `npm run build`, and `cd src-tauri && cargo check`.
- Verify old task data loads with default active status.
- Create a task, start/pause/reset timer, mark done, reopen it.
- Schedule a task for 1 minute later with auto-start on and off.
- Snooze a fired schedule by 5 minutes and confirm it fires again.
- Finish a countdown and verify the finished state offers useful next actions.
- Delete a task and verify confirmation or undo prevents accidental loss.
- Toggle global pause and confirm list/detail both show paused state.
- Schedule a task with Telegram enabled and confirm the "Task scheduled" message includes the formatted due time.
- Let a scheduled task fire with auto-start on and off; confirm Telegram copy differs correctly.
- Finish a countdown and confirm the timer-finished template is sent.
- Use Settings `Send test` and confirm the connected template is sent.
- Verify titles/content with `<`, `>`, and `&` are escaped correctly.

## Assumptions

- Direction is **Minimal Utility**: improve clarity and flow without making Notch feel like a full todo app.
- Done tasks are hidden by default, not deleted.
- Snooze creates a new one-shot schedule for the same task.
- Telegram remains one-way notification only; no bot commands or inline buttons in this pass.
- No cloud sync, calendar integration, projects, tags, or AI features in this UX pass.
- Visual style stays close to the current sticky-note identity, with better hierarchy and safer controls.
