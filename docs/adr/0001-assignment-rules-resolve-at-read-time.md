# 1. Assignment Rules resolve at read time, beneath Assignments

Date: 2026-07-25

## Status

Accepted. The title subject slot reserved below is filled by
[ADR 0003](./0003-title-rules-match-by-substring.md).

## Context

Assignments are dated: dropping an app on a project links it for the days on
screen and no others. Reviewing therefore means re-stating the same judgement
every day the app appears.

Measured on 42 days of real use: 45 assignment rows collapsing to 18 distinct
(app, title, project) tuples. One tuple — Warp to Flowstate at the app level —
had been re-stated **15 times** in five weeks. Meanwhile 14 of 30 tracked days
carried no assignment at all, holding 21.3 reviewable hours; roughly 16 of
those hours belong to four apps that would each be a one-line standing rule.
The cost of reviewing grows with every day tracked, and it had already outrun
the willingness to pay it.

## Decision

An **Assignment Rule** is a standing instruction — *"this activity belongs to
this project, always"* — resolved when time is read, never written into
per-day records.

**Rules sit beneath Assignments.** Resolution walks four rungs and stops at the
first that speaks:

```
day-title  >  day-app  >  rule-title  >  rule-app
```

A dated, deliberate act always outranks a standing policy, at any specificity.

**A rule's constraints are named slots combined with AND**, and they divide
into two kinds:

- **Subject** — app, and optionally window title. Subject decides which rung a
  rule occupies, mirroring the precedence Assignments already have.
- **Gate** — weekday, time of day, effective-from date. A gate can stop a rule
  from applying; it never wins an argument between rules.

Within one rung, every matching rule applies and each bills the full duration —
the same overlapping-totals behaviour two Assignments on one row already have.
One rule targets one project; overlap is expressed as two rules.

**Rules are retroactive by default**, with a per-rule forward-only choice taken
when the rule is saved. Deleting a rule un-bills its history symmetrically.

Version one ships the app subject slot only. (Superseded by ADR 0003, which
fills the title slot with a case-insensitive substring pattern.)

## Consequences

Writing four rules reclassifies roughly three quarters of the existing
backlog immediately, and the daily drag disappears for anything a rule covers.

Time attribution stays start-of-segment, as everywhere else in Echo. A
time-of-day gate could in principle need to split a segment across its
boundary; measured against real data it would misfile 112 of 13,722 segments
(0.8%), only 35 of them longer than five minutes. Not worth splitting segments
for. This is the reason gates were kept out of precedence: they narrow *when*,
so they can never force a segment to belong to two rungs at once.

The subject/gate split is what keeps the ladder at four rungs permanently.
Adding weekday, time of day, or effective-from later is a nullable column and
a condition — the same shape as the four migrations already in `db.rs` — and
touches no precedence logic.

Deliberately left for later, each admitted by this design without rework:
title subject slots, gate slots beyond effective-from, a dry-run preview of a
rule's blast radius (worth building when title patterns arrive and reach stops
being guessable), creating rules from the day view, and Echo proposing rules it
notices you repeating.

Rules are never "locked in" by age — a total from June can change today. The
answer to that, should it ever bite, is not forward-only rules but promoting a
rule's output into real Assignments, which would reintroduce exactly the
duplication this decision removes.

## Alternatives rejected

**Make Assignments sticky instead** — dragging means "from now on." No new
concept, but it destroys the ability to say "just today," and 13 of the 18
observed tuples were genuine one-offs (a pull request page, a benchmark
article) that would have become permanent rules nobody wanted.

**Materialise rules into per-day records as time accrues** — leaves the read
path untouched, but makes the assignment table a mixture of hand-made and
derived rows with no way to tell them apart, orphans rows when a rule is
deleted, and cannot classify history.

**Specificity-first precedence** (`day-title > rule-title > day-app > rule-app`)
— one law instead of two, and genuinely more elegant. Rejected because a rule
written months ago would silently outrank a drag performed seconds ago, which
is the fastest way to stop trusting the numbers.

**Rule precedence by counting filled constraints** — makes *Warp + weekdays*
outrank *Warp* by arithmetic accident rather than intent.

**User-ordered rules, first match wins** — utterly predictable, and tempting
given projects already carry a sort order. Rejected because it makes overlap
impossible through rules, and overlapping project totals are deliberate here.

**A bag of criteria, or an opaque stored predicate** — open to matcher kinds
nobody has asked for, at the cost of a rule no longer being a row you can read
and understand. The set of useful matchers is small and already nameable.
