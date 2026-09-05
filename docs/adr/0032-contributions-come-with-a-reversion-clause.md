# ADR-0032: Contributions come under a fiduciary agreement with a reversion clause

- **Status:** Accepted as a direction. **Not in force**: no agreement text is
  adopted or signed by this ADR, and until one is, `CONTRIBUTING.md` continues
  to ask only for the DCO.
- **Date:** 2026-09-05
- **How it was decided:** by management, in this repo, before the project takes
  its first external contribution. The shape was given precisely: *"me gusta
  CLA pero para la doble licencia pero quiero que la primera sea siempre AGPL
  aunque cambie de decisión yo. Poner como que no se puede relicenciar si no
  sacar bajo otras licencias según lo que considere"* — additional licences
  yes, closing the public one no, and not even by its own owner later.
  Everything asserted about this repo is **verified by reading it** and cited.
  **The account of the FLA-2.0 is from secondary sources read in this session
  and is not legal advice** — see §What this ADR is not.
- **Relates to:** ADR-0004, `CONTRIBUTING.md`, [licencia.md](../licencia.md)
- **Scope:** decides **what is asked of a contributor and what is promised in
  return**. It does **not** change the licence of anything shipped today, does
  **not** adopt a specific agreement text, and does **not** touch the AGPL /
  Apache split that ADR-0004 decided.

## Context

Today `CONTRIBUTING.md:92-102` asks for the DCO and says, in as many words,
*"No se exige CLA en esta fase"* — a door left open on purpose.

The DCO is not what it is often taken for. It certifies that whoever commits
has the right to contribute under the project's licence; it transfers nothing.
Every contributor keeps their copyright, and the moment the first external pull
request is merged, Anvil becomes a work with several rightsholders. From then
on nothing can be relicensed — no commercial licence, no change of terms —
without the permission of each of them, individually, forever.

That collides with a problem ADR-0004 already named. Its own context states
that companies commonly forbid AGPL in their dependencies and that the licence
choice therefore affects industrial adoption directly. Anvil's answer so far is
that *using* Anvil triggers no AGPL obligation, which is true and covers most
of the field — but not the customer who wants to modify it, or ship it inside
something of their own, and cannot take AGPL terms at all.

**And there is a window.** There are no external contributors yet, so agreeing
this now costs nothing. After the first outside merge, every person who does
not sign is code to chase or rewrite.

The obvious instrument for this is a CLA, and the obvious objection to a CLA is
that it is asymmetric: the owner may relicense the contributor's work, and the
contributor may not relicense the owner's. In AGPL projects it is read, with
some history behind it, as *"free software until it stops being convenient"* —
Elastic, MongoDB, Redis and HashiCorp each ended up changing terms, and a CLA
is what let them.

## Decision

**Contributions are taken under a fiduciary agreement that lets Anvil grant
*additional* licences, and that binds Anvil to keep publishing under AGPL —
with the rights reverting to their authors if it ever stops.**

1. **The public Anvil is AGPL, and that is not the owner's to revoke later.**
   This is the whole point of the shape chosen: the commitment binds the
   project against its own future self, not only against a hypothetical buyer.
   A decision to close Anvil is not a decision that remains available.

2. **A reversion clause makes it real rather than a promise.** If the fiduciary
   acts against the principles of free software, the granted rights return to
   the contributors who granted them. A commitment nobody can enforce is a
   press release; this is the term that gives it teeth, and it points at the
   owner.

3. **Additional licences are permitted, and are the reason for the agreement.**
   Anvil may license the same code to a customer on other terms — the
   industrial buyer that cannot take AGPL. That is a second lane beside the
   AGPL one, never instead of it.

4. **Authorship stays with the author**, named in the history and in the
   commit trailers. Moral rights are not transferable under Spanish law in any
   case; what moves is exploitation, and it moves under condition 2.

5. **The starting point is the FSFE's FLA-2.0**, rather than a text written
   here. It is maintained by people who do this for a living, it is used by
   KDE e.V. among others, and it already contains the two pieces this decision
   needs: the binding promise that contributions stay free software, and the
   reversion clause. FLA-2.0 in particular adds patent coverage and explicit
   provision for licensing to third parties, which is exactly point 3.

6. **Nothing changes until the text is adopted.** The DCO stays as the only
   requirement in the meantime, and `CONTRIBUTING.md` says so. Asking people to
   sign something that does not exist yet is worse than asking nothing.

7. **"Dual licence" is not the name for this.** In this repo that phrase
   already means the AGPL-product / Apache-libraries split of ADR-0004, and
   reusing it would repeat the collision `docs/glosario.md` has just had to
   untangle over the word *bridge*. This is the **commercial lane**, and the
   AGPL/Apache split keeps the name it has.

## What this ADR is not

**It is not legal advice, and no part of it was written by a lawyer.** The
description of the FLA-2.0 above comes from secondary sources read on
2026-09-05, not from reading the instrument in full or from any professional
opinion. Adopting it — or any agreement — needs a lawyer's review of the actual
text against Spanish law and against ANLACO's situation, and this ADR records a
direction so that review has something specific to review.

Two questions in particular are outside what was settled here and should be put
to whoever reviews it: what "acts against the principles of free software" is
taken to mean concretely enough to be enforceable, and whether reversion is
workable at all once the work is genuinely collective.

## Alternatives discarded

**Stay on the DCO alone.** The strongest signal of good faith available, and it
closes the commercial lane permanently — not by decision but by arithmetic,
since unanimity among past contributors is unobtainable in practice. Rejected
because ADR-0004 already identified the customer this shuts out.

**A plain CLA, Apache ICLA style, with no reversion.** Simpler, extremely well
understood, and it grants everything needed for the commercial lane. Rejected
on management's explicit instruction: it leaves closing the project available
to a later owner, and the point here was to remove that option.

**Copyright assignment with no strings, FSF style.** Maximum freedom for the
owner, maximum friction for contributors, and it discards the reversion that
makes this palatable at all.

**A public promise with no legal force.** A README section saying Anvil will
always be AGPL. Costs nothing, binds nothing, and is worth exactly what the
same promises were worth at the companies listed in §Context.

## Consequences

- **Contribution gets a barrier it does not have today.** Some people will not
  sign, and that cost is real and should not be argued away. Being able to
  point at the reversion clause is what makes it arguable at all.
- **Work is owed before this is in force**: a reviewed text, a signing flow
  that does not require paper, and a record of who signed. `CONTRIBUTING.md`
  keeps saying the DCO is all that is required until then.
- **The DCO does not go away.** It answers a different question — did you have
  the right to contribute this — and both are normally asked together.
- **The commercial lane becomes possible, not automatic.** Nothing about
  pricing, packaging or what a commercial licence would even say is decided
  here, and none of it should be assumed from this.
- **`docs/licencia.md` becomes incomplete** the day this takes effect: it
  describes the AGPL/Apache split as the whole licensing strategy, and it will
  need the second lane written into it.
