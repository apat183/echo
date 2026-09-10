# 3. Title Rules match by substring, and shadow the App Rule

Date: 2026-07-26

## Status

Accepted. Fills the title subject slot ADR 0001 reserved and left empty.

## Context

ADR 0001 shipped rules with an app subject only, and reserved rung three of the
ladder (`day-title > day-app > rule-title > rule-app`) for a title subject.

An app subject is too coarse for the apps that carry the most time. A browser,
an editor and a terminal are each one app key covering several different kinds
of work; a rule can only send all of it to one project, so the review keeps
coming back to exactly the apps that generate the most rows.

The obstacle is the nature of the subject. `CONTEXT.md` already records it:
window titles are *"high cardinality and mostly transient — a browser tab title
is usually seen once and never again."* An exact title is not a stable thing to
write a standing rule about. `"Inbox (42) - Gmail"` is dead by the next email;
`"echo — src/db.rs"` is dead by the next file.

## Decision

A Rule's subject gains an optional **Title Pattern**: `""` means the whole app,
anything else is matched against the window title **case-insensitively, as a
substring**. `assignment_rules` gets one nullable-by-default column; nothing
else about a rule changes, gates included.

**A matching Title Rule shadows the App Rules of the same app.** Resolution
still stops at the first rung that speaks, exactly as ADR 0001 laid down: if any
title rule matches, rung three answers alone and rung four stays silent. Within
rung three every matching rule applies and each bills the full duration, as
within any other rung.

Because a pattern is never empty, untitled activity can never match one and
always falls through to the app rules.

The Rules pane offers the app's real titles, busiest first, to **seed** a
pattern that the user then trims to the stable part — and shows the pattern's
**reach** (how many tracked titles it matches, and for how long) before the rule
is saved. This is the dry-run preview ADR 0001 deferred "until title patterns
arrive and reach stops being guessable". It is computed from the same title list
the picker already loads, so it costs no additional query.

Title Rules draw on the **title rows** of the day view, not on the app row. An
App Rule's subject is the app, so it draws on the app row; a Title Rule's
subject is the title. Without this a shadowed title would render as blank while
billing to a project — an Included link that looks Undecided, which ADR 0002
forbids.

Rules stay add-and-delete; there is no edit. Tuning a pattern is delete and
retype, and the reach preview is what makes retyping cheap.

## Consequences

The apps worth splitting can now be split, without the per-day drag rules exist
to remove.

An **Exception is still dated and keyed to the exact row** (`date`, `app_key`,
`title`), never to the pattern. Clicking the dot off a rule-covered title
removes it for that title on that day only; the rule keeps standing everywhere
else. This is ADR 0002's provenance-blindness working as designed — but it does
mean a pattern that is *mostly* right is corrected day by day, and a pattern
that is *mostly wrong* should be deleted rather than excepted. The reach preview
is the guard against writing the second kind.

Matching costs one lowercase allocation per segment, and only for apps that
actually carry a title rule; apps without one probe a map and return. If a
pattern set ever grows large enough for the linear scan within an app to show
up, the upgrade is an Aho-Corasick pass, not a change to the model.

`untagged_segment_ids` reads through the same `Resolver`, so auto-delete
automatically stops eating anything a title rule now classifies.

Deliberately still not built: gate slots beyond `effective-from`, creating rules
from the day view, editing a rule in place, and Echo proposing patterns it
notices you repeating.

## Alternatives rejected

**Exact title match** — the smallest change, and consistent with how
`day_assignments` keys a title-level assignment. Rejected because a dated
assignment is *about one day*, where a transient title is a perfectly good key,
while a rule is *about all days*, where it is not. Measured against real titles,
almost every exact rule would be dead within a week of being written.

**Glob or regex** — maximum reach, and the only option that expresses "starts
with" or alternation. Rejected on ADR 0001's own grounds: a rule must stay a row
you can read and understand, and a regex brings pattern validation, an error
state in the form, and a rule nobody can debug at 3am. Substring covers the
observed need; regex is a strictly later, additive change if it ever bites.

**Stacking instead of shadowing** — a matching title rule bills *in addition to*
the app rule, mirroring how two Assignments on one row both bill. Rejected
because it makes the app rule inescapable: the only way to say "this app is
Reading, except the ScriptR tabs" would be a per-day Exception forever, which is
the cost rules were introduced to eliminate. Overlap remains expressible — write
two title rules.

**Specificity by pattern length**, so a longer pattern outranks a shorter one
within rung three. Rejected for the same reason ADR 0001 rejected counting
filled constraints: it decides precedence by arithmetic accident rather than
intent, and overlapping totals are deliberate here anyway.
