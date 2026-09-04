# Desktop Notification Capture — Implementation Contract

Frozen interface between the Rust backend and the React frontend. Both sides
build against this; neither redefines it unilaterally.

## Goal

Pomodoro observes desktop notifications delivered by other applications, files
them in a triage list inside the app, and lets the user declare which kinds
matter. Notifications that arrive mid-focus become reviewable *after* the
session instead of stealing attention during it.

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
}

pub struct NotificationFilter {
    pub enabled: bool,               // master capture switch, default false
    pub min_urgency: u8,             // drop anything below, default 0
    pub muted_apps: Vec<String>,     // never capture (case-insensitive match)
    pub priority_apps: Vec<String>,  // always capture, bypasses min_urgency
    pub focus_only: bool,            // only capture during focus, default false
}
```

`NotificationFilter` lives on `Settings` as `notificationFilter`.
`Vec<DesktopNotification>` lives on `AppData`/`AppSnapshot` as `notifications`.

### Filter precedence (implement exactly, and test)

1. `enabled == false` → drop everything.
2. `app_name` matches `muted_apps` → drop. **Mute wins over priority.**
3. `app_name` matches `priority_apps` → keep, ignoring rules 4 and 5.
4. `focus_only == true` and no focus interval running → drop.
5. `urgency < min_urgency` → drop.
6. Otherwise keep.

Cap retention at **200** notifications, newest first; drop the oldest beyond
that so the JSON store cannot grow without bound.

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
| `convert_notification` | `id: String` | Create a task titled from `summary`, mark triaged |
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
`Settings.notificationFilter: NotificationFilter`

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
