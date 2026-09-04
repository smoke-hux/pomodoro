# Desktop Notification Capture — Implementation Contract

Frozen interface between the Rust backend and the React frontend. Both sides
build against this; neither redefines it unilaterally.

## Goal

Pomodoro observes desktop notifications delivered by other applications, files
them in a triage list inside the app, and lets the user declare which kinds
matter. Notifications that arrive mid-focus become reviewable *after* the
session, so nothing has to be dealt with while the interval is running.

### What monitoring is not

A bus monitor is a passive observer. By the time a `Notify` call is seen the
desktop has already accepted it and drawn the banner, so capture **cannot**
suppress anything: banners still appear and sounds still play. Any copy that
implies otherwise is wrong and must be corrected.

Going quiet is a separate, opt-in feature — see *Silencing banners* below — and
it works by asking the desktop, not by intercepting the bus.

## Verified platform facts

Established by probe on this machine, not assumed:

- Session bus: `unix:path=/run/user/1000/bus`
- Daemon: `gnome-shell`, GNOME, server spec 1.2
- A monitor connection **does** receive the full `Notify` payload from other
  processes.

`Notify` signature (org.freedesktop.Notifications):

| # | Type | Meaning |
|---|------|---------|
| 0 | `s` | app_name |
| 1 | `u` | replaces_id |
| 2 | `s` | app_icon |
| 3 | `s` | summary |
| 4 | `s` | body |
| 5 | `as` | actions |
| 6 | `a{sv}` | hints (`urgency` byte: 0 low, 1 normal, 2 critical) |
| 7 | `i` | expire_timeout |

### Gotcha — the same notification is seen twice

The probe captured each notification twice: once `sender→gnome-shell`, then
again `gnome-shell→<other monitor>` as the shell relays it. **Deduplicate**, or
every notification is filed twice. Suggested rule: accept only the first
delivery within a short window keyed on `(app_name, summary, body)`, or ignore
calls whose sender is the notification daemon itself. Cover this with a test.

The implemented key is `(app_name, summary, body, replaces_id)`, length-prefixed
so text containing the separator cannot forge a collision.

### Gotcha — `replaces_id` is an update, not a new notification

A sender that passes a non-zero `replaces_id` is updating a notification it
posted earlier: a download counting up, a call still ringing. Filing each of
those as its own row lets one chatty sender bury the inbox. Match an incoming
call against `(app_name, replaces_id)` and update that row in place instead —
refreshing its text, urgency and timestamp, and clearing `triaged` only when the
words actually changed. `replaces_id == 0` always means "new".

### Gotcha — an expired interval is still `Running`

The timer keeps `TimerStatus::Running` between the moment it runs out and the
next reconciliation tick, up to 500 ms later. `during_focus` must be decided
from the remaining time at the notification's own timestamp, not from the status
alone, or notifications belonging to the break get filed as focus interruptions.

### Gotcha — Pomodoro's own notifications are on the same bus

The app's boundary alerts (`Focus complete`, `Break complete`) go out through the
same service. Drop them by app name before the filter runs, so that not even a
priority-list entry can let them in.

## Rust domain types

`serde` must use `rename_all = "camelCase"` — an existing test asserts the
TypeScript contract.

```rust
pub struct DesktopNotification {
    pub id: String,          // "notif-<epoch_ms>-<counter>"
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub urgency: u8,         // 0 low | 1 normal | 2 critical
    pub received_at: i64,    // epoch ms
    pub during_focus: bool,  // captured while a focus interval was running
    pub triaged: bool,       // user has dealt with it
    pub replaces_id: u32,    // sender's Notify replaces_id; 0 means "new"
    pub task_id: Option<String>, // set once converted, so it cannot convert twice
}

/// Runtime health of the monitor. Serialized for the UI, never read back from
/// disk: a monitor that ran last time says nothing about this run.
pub struct CaptureStatus {
    pub state: CaptureState, // off | starting | active | failed
    pub detail: String,      // D-Bus error when failed; never notification text
}

pub struct NotificationFilter {
    pub enabled: bool,               // master capture switch, default false
    pub min_urgency: u8,             // drop anything below, default 0
    pub muted_apps: Vec<String>,     // never capture (case-insensitive match)
    pub priority_apps: Vec<String>,  // always capture, bypasses min_urgency
    pub focus_only: bool,            // only capture during focus, default false
}
```

Text limits, applied before storing: `app_name` 128 characters, `summary` 512,
`body` 4096, truncated on character boundaries. A sender can put an arbitrarily
long string in any of them.

`NotificationFilter` lives on `Settings` as `notificationFilter`.
`Vec<DesktopNotification>` lives on `AppData`/`AppSnapshot` as `notifications`.

### Filter precedence (implement exactly, and test)

1. `enabled == false` → drop everything.
2. `app_name` matches `muted_apps` → drop. **Mute wins over priority.**
3. `app_name` matches `priority_apps` → keep, ignoring rules 4 and 5.
4. `focus_only == true` and no focus interval running → drop.
5. `urgency < min_urgency` → drop.
6. Otherwise keep.

Rule 0, before all of the above: a notification whose `app_name` is Pomodoro's
own is dropped outright.

Cap retention at **200** notifications, newest first; drop the oldest beyond
that so the JSON store cannot grow without bound.

## Persistence

Captured notifications are **debounced**: filing marks the store dirty and the
reconciliation loop writes at most once every 2000 ms. A burst — a group chat
waking up, a build reporting every step — must not rewrite the whole JSON store
dozens of times a second from the monitoring thread. User actions still save
immediately, and a pending write is forced on quit.

The store is written owner-only: the data directory `0700`, the file `0600`,
applied on every save so a store from an earlier build is repaired.

## Capture status

Turning capture on is a request, not a guarantee. The session bus can refuse
`BecomeMonitor`, and there may be no session bus at all. The listener reports
`starting`, then `active` or `failed`, and the UI reads that rather than the
settings toggle — it must never claim to be watching when nothing is. The status
is runtime state: serialized outward, `skip_deserializing` on the way in.

## Silencing banners

`Settings.silence_banners_during_focus`, default **false**. While a focus
interval is running, set GNOME's `org.gnome.desktop.notifications show-banners`
to `false`; restore it when the interval ends, on quit, and at the next launch
if the app was killed in between. The value to restore is persisted on `AppData`
as `banner_restore` precisely so a crash is recoverable.

Constraints:

- A user who had already switched banners off is left alone. There is nothing to
  restore and nothing to take credit for.
- Every failure is soft. No `gsettings`, no GNOME, a locked-down schema — the
  desktop simply does not go quiet, and the timer and capture are unaffected.
- It is a distinct setting from capture, and works whether or not capture is on.
  It changes something that belongs to the desktop, so it is never implied by
  another switch.

## Privacy rule — non-negotiable

Notification bodies routinely contain message contents, 2FA codes, and email
subjects. This data:

- stays in the existing local JSON store, and is written nowhere else
- is never logged to stdout/stderr, including in debug builds
- is never transmitted anywhere — the app makes no network requests

Capture defaults to **off**. The user opts in explicitly in Settings.

## IPC commands

Every command returns the full `AppSnapshot`, matching the existing convention,
and broadcasts `state-changed`.

| Command | Args | Effect |
|---------|------|--------|
| `set_notification_filter` | `filter: NotificationFilter` | Persist filter; start/stop the listener to match `enabled` |
| `triage_notification` | `id: String`, `triaged: bool` | Mark handled |
| `convert_notification` | `id: String` | Create a task titled from `summary`, mark triaged. **Idempotent**: a second call returns the task the first one made rather than a duplicate |
| `delete_notification` | `id: String` | Remove one |
| `clear_notifications` | – | Remove all |

## TypeScript mirror (`src/types.ts`)

```ts
export interface DesktopNotification {
  id: string;
  appName: string;
  summary: string;
  body: string;
  urgency: 0 | 1 | 2;
  receivedAt: number;
  duringFocus: boolean;
  triaged: boolean;
  replacesId: number;
  taskId: string | null;
}

export type CaptureState = "off" | "starting" | "active" | "failed";

export interface CaptureStatus {
  state: CaptureState;
  detail: string;
}

export interface NotificationFilter {
  enabled: boolean;
  minUrgency: 0 | 1 | 2;
  mutedApps: string[];
  priorityApps: string[];
  focusOnly: boolean;
}
```

`AppSnapshot.notifications: DesktopNotification[]`
`AppSnapshot.captureStatus: CaptureStatus`
`Settings.notificationFilter: NotificationFilter`
`Settings.silenceBannersDuringFocus: boolean`

## Backward compatibility

Existing `pomodoro.json` files predate these fields. Deserialization **must**
use `#[serde(default)]` so an older store still loads. A user's existing tasks
and history must survive the upgrade — verify by loading a store lacking both
new fields.

## Design constraints

Follow `docs/brand-guidelines.md`. Specifically:

- Voice: calm, plain, no exclamation marks, no praise. Name consequences.
- Ember = work, olive = rest. Do not introduce a new accent hue.
- Colour is never the sole carrier of meaning — urgency needs a text label too.
- Destructive actions confirm and say what is lost.
- Honour `prefers-reduced-motion` and `forced-colors`.
- Layout holds at the 760×560 minimum window.
- Space belongs to whatever control has focus. The window-level timer shortcut
  yields to buttons, links, form fields, `<summary>`, and anything carrying an
  interactive ARIA role.
- Settings edits belong to the user until they save. The dialog seeds its draft
  once, on open, and never from the state broadcast that arrives twice a second
  while the timer runs.
