# Brand Guidelines v1.0

> Last updated: 2026-09-02
> Status: Active
> Product: Pomodoro — a local-first focus timer for Ubuntu

## Quick Reference

| Element | Value |
|---------|-------|
| Primary Color | #98452B |
| Secondary Color | #667044 |
| Accent Color | #DD825D |
| Primary Font | Ubuntu Sans |
| Voice | Calm, Plain, Unhurried |

---

## 1. Brand Position

Pomodoro is a **quiet instrument**, not a productivity coach. It counts time and
keeps an honest record. It never gamifies, congratulates, guilt-trips, or nags.

The competitive field is loud — streaks, badges, confetti, "You're on fire!". The
whole position is the opposite: a tool calm enough to sit on screen for eight
hours without becoming a second source of stress. Every design decision resolves
toward **lower arousal**.

### Design Principles

1. **The clock is the interface.** One number dominates. Everything else recedes.
2. **Honest ledgers.** A skipped session reads as skipped. Credit is never
   inflated to make the day look better.
3. **Interruptions are captured, not punished.** Noting a distraction takes one
   keystroke and never stops the timer.
4. **Silence is the default state.** The app speaks only at a phase boundary or
   after a user action, then goes quiet.
5. **Nothing is lost to a misclick.** Destructive actions confirm; data is local
   and durable.

---

## 2. Color Palette

The palette is warm and low-contrast-fatigue: paper-cream grounds with earth
pigments, rather than the blue-white glare typical of timer apps. Hue carries
meaning — **ember = work, olive = rest** — so phase is legible at a glance,
peripherally, without reading a label.

### Primary Colors

| Name | Hex | RGB | Usage |
|------|-----|-----|-------|
| Ember | #98452B | rgb(152,69,43) | Focus phase, primary action, progress fill |
| Ember Dark | #7D351F | rgb(125,53,31) | Hover and pressed states |

### Secondary Colors

| Name | Hex | RGB | Usage |
|------|-----|-----|-------|
| Olive | #667044 | rgb(102,112,68) | Break phases, rest states |
| Olive Dark | #535C37 | rgb(83,92,55) | Break hover and pressed states |

### Accent Colors

| Name | Hex | RGB | Usage |
|------|-----|-----|-------|
| Ember Light | #DD825D | rgb(221,130,93) | Dark-theme primary, highlights |

### Neutral Palette

| Name | Hex | RGB | Usage |
|------|-----|-----|-------|
| Canvas | #F2EEE7 | rgb(242,238,231) | Application ground |
| Surface | #FBF8F3 | rgb(251,248,243) | Panels, cards |
| Surface Raised | #FFFDF9 | rgb(255,253,249) | Dialogs, menus |
| Text Primary | #25211D | rgb(37,33,29) | Headings, body, the clock |
| Text Secondary | #6B635B | rgb(107,99,91) | Captions, metadata |
| Divider | #D7D0C7 | rgb(215,208,199) | Rules, borders |

### Semantic Colors

| State | Hex | Usage |
|-------|-----|-------|
| Success | #667044 | Completed sessions, handled interruptions |
| Warning | #98452B | Over-estimated tasks, capacity limits |
| Error | #9B3632 | Destructive confirmation, load failures |
| Info | #6B635B | Neutral notices |

### Accessibility

Measured against the WCAG 2.1 relative-luminance formula:

| Pair | Ratio | Grade |
|------|-------|-------|
| Text Primary on Canvas | 13.0:1 | AAA |
| Text Secondary on Canvas | 5.10:1 | AA |
| White on Ember | 6.53:1 | AA |
| White on Olive | 5.30:1 | AA |

Hue is never the sole carrier of meaning: every phase colour is paired with a
text label, and every state change is announced to the live region.

---

## 3. Typography

### Font Stack

```css
--font-body: "Ubuntu Sans", Cantarell, system-ui, sans-serif;
--font-clock: "Ubuntu Sans", Cantarell, system-ui, sans-serif;
```

Ubuntu Sans is the system face on the target platform, so it loads instantly,
matches the surrounding desktop, and ships no webfont. **Do not add a webfont** —
an offline-first local app must not make a network request to render.

### Type Scale

| Element | Size | Weight | Line Height | Notes |
|---------|------|--------|-------------|-------|
| Clock | 76px | 300 | 1.0 | Tabular numerals, never reflows |
| H2 (dialog title) | 19px | 600 | 1.3 | |
| Body | 14px | 400 | 1.45 | Base |
| Small | 13px | 400 | 1.45 | Metadata, ledger rows |
| Caption | 12px | 400 | 1.4 | Shortcut hints |

The clock uses `font-variant-numeric: tabular-nums` so digits hold their column
and the display does not jitter each second.

---

## 4. Voice

**Calm, Plain, Unhurried.**

The app addresses a person mid-concentration. Every string is read at a glance,
in peripheral vision, by someone who does not want to be talking to software.

### Rules

- **State facts, don't cheer.** "Focus ended without credit." not "Aw, you gave up!"
- **No exclamation marks.** Anywhere.
- **No streaks, scores, or praise.** The ledger is the feedback.
- **Second person, present tense, for instructions.** "Note it, then return to focus."
- **Name the consequence in destructive prompts.** Say what is lost, not "Are you sure?"
- **Sentence case everywhere**, including buttons.

### Voice Table

| Situation | Write | Don't write |
|-----------|-------|-------------|
| Focus completes | Focus complete. Take a short break. | Great job! You crushed it! |
| Focus skipped | Focus ended without credit. | Session abandoned :( |
| Interruption saved | Saved. Return to your focus. | Got it! Logged! |
| Nothing planned | No tasks yet. | Your task list is empty! Add one! |
| Load failure | Pomodoro could not open its local data. | Oops! Something went wrong! |
| Over-estimate | Estimated above four sessions. Consider splitting it. | That's too big! |

---

## 5. Motion

Motion communicates state change and nothing else. There are no decorative
animations.

| Element | Duration | Easing |
|---------|----------|--------|
| Progress fill | 160ms | linear |
| Control hover | 120ms | ease-out |
| Dialog entry | 140ms | ease-out |
| Paused pulse | 2400ms | ease-in-out, infinite |

All motion collapses under `prefers-reduced-motion: reduce`. The paused pulse
degrades to a static outline — the state stays legible without movement.

---

## 6. Consistency Checklist

Before shipping a UI change:

- [ ] Phase colour matches phase semantics (ember = focus, olive = rest)
- [ ] Every colour-coded state also carries a text label
- [ ] Strings pass the voice table — no praise, no exclamation marks
- [ ] Destructive actions name what is lost and require confirmation
- [ ] Contrast ≥ 4.5:1 for body text, ≥ 3:1 for UI borders
- [ ] Keyboard path works, and focus is visible at every stop
- [ ] Layout holds at 760×560 (the enforced minimum window)
- [ ] Verified in light and dark
