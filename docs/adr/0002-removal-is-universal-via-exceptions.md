# 2. Removal is universal, recorded as provenance-blind Exceptions

Date: 2026-07-25

## Status

Accepted. Extends [ADR 0001](0001-assignment-rules-resolve-at-read-time.md).

## Context

Once activity can be linked to a project by something other than a direct act
— an App-Level Assignment covering a title, or an Assignment Rule covering an
app — the obvious gesture breaks. Clicking a project dot deletes a row
(`DayPane.tsx:260`); an inherited link has no row to delete.

Echo already met this problem and answered it by taking the affordance away.
`project_apps` computes `can_remove` as "this title has an explicit link of its
own" (`db.rs:839-840`) and `ProjectPane.tsx:402` hides the remove button when it
is false. The consequence, live today: app-level assign a browser to a project
and no single tab can be taken back out. Against 1,769 distinct titles in real
data, that is not a corner case.

Rules make the same problem systematic rather than occasional.

## Decision

**Any link that can be seen can be removed**, whether it was made directly or
inherited, and whether it was inherited from an App-Level Assignment or from a
Rule. `can_remove` is retired.

Removal that has nothing to delete records an **Exception**: a dated statement
that specific activity is *not* in a project on that date. An Exception is
**blind to provenance** — it records no link to the rule or assignment whose
effect it cancels — so it stays meaningful when that source is later edited,
deleted, or converted between the two.

On a given day, activity stands in exactly one of three relations to a project:
**Included**, **Excluded**, or **Undecided**. Assignment and Exception are
mutually exclusive for the same key: recording one clears the other. Excluded
is a judgement already made; Undecided is a gap still waiting for one, and the
two must never render alike.

**Clicking an Included dot takes back what put it there and stops it coming
back**: delete any direct assignment, then record an Exception only if the
activity would still be inherited. Clicking an Excluded dot returns it to
Undecided, which lets the rules apply again.

Dots carry their provenance: filled for a direct assignment, ringed for a rule,
struck through for an Exception, with the responsible rule named in the tooltip.

## Consequences

Ordinary unassign behaves exactly as it does today — a hand-made link with no
rule behind it vanishes on click and stores nothing. The third state only
becomes visible on activity a rule is holding, which is precisely where it is
needed. On rule-covered rows the gesture collapses to a plain on/off toggle,
because Undecided and Included render identically when a rule covers the
activity.

Provenance-blindness is what makes an Exception durable. Had exceptions
referenced the rule that caused them, editing that rule would orphan them. The
requirement fell out of universality: an Exception must equally be able to
cancel an App-Level Assignment, which has no rule to point at.

Because a link may now exist for reasons the user cannot see, the project view
must be able to decompose a day into the items that made it up and say why each
one is there. That per-day receipt is part of this decision, not a separate
nicety — without it a total is a number with no way to interrogate it.

Deleting a project must sweep its Exceptions as well as its Assignments and
Rules; `delete_project` already does the equivalent for assignments
(`db.rs:317-320`).

Ignores are untouched and continue to outrank everything: ignored activity is
filtered before resolution begins (`db.rs:833`) and no rule can resurrect it.

## Alternatives rejected

**Exceptions for rules only, keeping `can_remove` for app-level inheritance** —
smaller change, nothing shipped moves. Rejected because it leaves no principle
behind the split: the remove button would work on an inherited title when the
parent was a rule and be absent when the parent was yesterday's drag, purely as
an artefact of which feature shipped first.

**A polarity flag on assignment records** rather than a distinct concept. Less
machinery, and safer than first argued — every reader already goes through a
single resolution gatekeeper rather than touching rows directly. Rejected on
domain grounds: "this is in the project" and "this is deliberately not in the
project" are different statements, and collapsing them into one record with a
sign invites readers to forget the sign.

**Letting an Exception and an Assignment coexist, Exception winning** — makes a
standing "no" outrank a deliberate dated act, contradicting the precedence
principle adopted in ADR 0001.

**Letting them coexist, Assignment winning** — leaves a dormant "no" on disk
that resurfaces if the assignment is later removed, so a row can silently
revert to excluded because of something recorded weeks earlier.

**A labelled menu on each dot** (*"Not this day"* / *"Clear"*) instead of an
overloaded click. Safest for a stranger, and the fallback if the click model
proves ambiguous. Rejected for now as a two-item menu behind every dot in a
dense list, for a single-user application.
