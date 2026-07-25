# Context

Echo's domain language. This file is a glossary, not a spec — it records what
the words mean, never how they are implemented.

## Tracking

**Segment** — one continuous stretch during which a single app, showing a single
window title, was frontmost. Segments are ground truth and are never revised
once recorded. A segment is billed entirely to the local calendar date on which
it started, even if it ran past midnight.

**App Key** — the stable identity of an app for the purposes of grouping and
matching. Two runs of the same app on different days share an app key; a rename
or version change does not break it.

**Window Title** — what the app's focused window was displaying. Best-effort:
some apps expose none, and Echo can be denied permission to read them, so
*untitled* is a legitimate value rather than an error. Titles are high
cardinality and mostly transient — a browser tab title is usually seen once and
never again.

## Review

**Project** — a bucket the user reviews time against. Projects are for personal
reflection, not billing: Echo has no concept of a client, rate, invoice, or
timesheet.

**Assignment** — a deliberate, dated act linking one day's activity to a
project. An assignment covers the day it was made and no other. Activity may
carry several assignments at once, and each one bills the full duration, so
project totals may legitimately overlap and sum to more than the day's tracked
time.

**App-Level Assignment** — an assignment made against an app rather than a
particular window title. It covers every title of that app that carries no
assignment of its own.

**Assignment Rule** — a standing instruction that activity matching some pattern
belongs to a project, holding on every day without a per-day act. Where an
Assignment says *"this, today"*, an Assignment Rule says *"this, always"*. Rules
are configured deliberately in one place rather than accumulated as a
side-effect of reviewing a day.

**Subject Constraint** — the part of a Rule that says what it is *about*: the
app, and optionally the window title. Subject is what gives one Rule authority
over another — a Rule about a particular title speaks more directly than a Rule
about the whole app, and so overrides it, exactly as a title-level Assignment
overrides an App-Level one.

**Gate Constraint** — the part of a Rule that says *when* it is live: a
weekday, a time of day, a date from which it takes effect. A gate can stop a
Rule from applying, but it never wins an argument between Rules. "Zen" and "a
tab called ScriptR" are different subjects; "on weekdays" is the same subject,
sometimes.

**Inherited Link** — a link between activity and a project that no one made
directly: it holds because an App-Level Assignment or an Assignment Rule covers
the activity. An inherited link is as real as a direct one — it bills the same
time and is shown the same way — and it can be taken back in the same way.

**Exception** — a dated statement that particular activity is *not* in a
project on that date, whatever would otherwise put it there. Where an
Assignment says *"this, today, yes"*, an Exception says *"this, today, no"*.
An Exception is deliberately blind to provenance: it does not record whether
the link it cancels was inherited from a Rule or from an App-Level Assignment,
so it stays meaningful when that source is later edited or deleted.

**Included / Excluded / Undecided** — on a given day, activity stands in
exactly one of three relations to a project. *Included*: it counts toward the
project, whether someone linked it directly or it was inherited. *Excluded*:
someone deliberately said it does not count, overriding whatever would have
included it. *Undecided*: nobody has said anything and nothing covers it.
Excluded and Undecided must never be confused — one is a judgement already
made, the other is a gap in the review still waiting for one. Clearing an
Exception returns activity to Undecided, which lets the Rules apply again.

**Ignore** — a standing instruction that matching activity is not activity at
all: it is excluded from review entirely rather than left unassigned. Distinct
from an Assignment Rule, which classifies activity rather than hiding it.

**Period Note** — a note the user attaches to a project for a given day, week,
or month.
