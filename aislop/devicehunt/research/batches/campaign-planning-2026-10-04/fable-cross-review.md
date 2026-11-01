*sets the three documents side by side, tail curled* okay. i read the synthesized plan, astra's review and my own proposal, then checked the manifest, `devicehunt/rra.py`, `devicehunt/capture.py` and the ised research collector only where a disagreement needed it. here's the challenge.

## verdict first

**ready for c0 and the c1 design contract, with specific edits.** none of the eight changes is wrong. four of them need a sharper statement before the c1 contract goes to review, and two carry scope risk worth trimming now. nothing here blocks pinning the pilot inputs.

## the eight changes, challenged

**1. one operational store with transactional reservations: accepted, with one invariant to add.** astra is right and i was wrong. `capture-leases.sqlite` keys leases per request and per host at `devicehunt/capture.py:174`, and the budget is instance memory at line 85. recounting tagged attempts after a host lease serializes nothing across hosts, so two runners on two regulators could each spend the whole campaign ceiling. evolving the lease store is the smallest fix. what the plan does not yet say is which record wins when the two disagree. it must be impossible for a `capture_attempt` receipt to exist without a committed reservation, and a reservation without an attempt receipt is by definition an uncertain attempt. name that ordering in the c1 contract. it is the whole crash story in one sentence.

**2. receipt count equality rejected: accepted.** my staleness check was a heuristic dressed as a protocol. one clarification is needed though. the plan says "start with one writer" and the c1 acceptance says two competing runners must be tested. those are compatible only if the test is about mutual exclusion, meaning the second runner is refused admission or lease, not about two processes writing the index at once. say so, or an implementer will build concurrent index writers to pass the test.

**3. release roots include coverage receipts, including empty and failed units: accepted, but it is a schema change, not a selection change.** `lineage.prepare` rejects an empty artifact set at `devicehunt/lineage.py:77` and every link must resolve to an artifact. a failed unit has no bytes. so a coverage-only or partly failed release cannot pass today's validator. c3 lists "lineage/catalog schema changes if needed". make it "needed": either a coverage-release record beside the evidence release, or a validator extension that accepts receipt-only roots. and label those roots as observation outcomes. a frozen "unavailable" receipt must never be readable later as evidence that a device does not exist.

**4. seven rra, five ised: accepted, verified.** the manifest has exactly those counts. my "eight spellings, nine bundles, ten captures" was wrong and should not survive anywhere.

**5. two comparison modes, distinct after observation: accepted, verified.** `exact_rows_ignore_order` keys on the certificate column at `rra.py:143`. the live point matters more than it looks: the csv export is a post with an identical body, so the same `request_id` would return `reused` unless the adapter passes `refresh=True`. the after export must also be its own budget reservation. two exports, two reservations, or the stability proof is manufactured from a cache hit.

**6. date windows are not a change feed: accepted, with a missing identity rule.** the importer derives the source id from the query digest including dates at `rra.py:173`. a revalidation window that overlaps an accepted window will create a second source containing the same certificates. the plan says revalidation is "separately bounded" but not what happens to the catalog. c0 must choose: overlapping windows are observation-only and never project to facts, or a revalidation is a complete re-export that replaces the old window's source. this is a c3 design gate, but c0 has to pick so the pin set is shaped right.

**7. lineage ordering: accepted. the no-bypass rule needs a scope cut.** today the importers are the gate, and they project whole sources. forcing every existing fact through a review queue is exactly the mass migration both reviews rejected. recommend: already-accepted sources keep their complete projection as the reviewed baseline, and the queue gates only units a campaign touches. otherwise c3 absorbs 45,017 facts of review work.

**8. session-aware reuse, not cookie-blind identity: accepted, and incomplete in a different direction.** the plan says cookies stay memory-only and receipts must not store them. but the attempt receipt persists request headers at `capture.py:197` and the result row persists response headers at line 211, so a `Set-Cookie` would land in a receipt today. redaction must cover both paths. separately, ised's webflow puts execution ids in the url and form body. those are not credentials, but they are session-bound. keep the wire url exact as evidence, let the adapter declare the step session-dependent so no reuse is attempted, and redact only cookie headers. decide that in c0, because it decides what the ised saved captures' receipts look like.

## other corrections worth making

- **restriction granularity.** `_blocked` matches 402/403 by exact url; throttle matches by host. "a new run cannot evade an existing restriction" needs to say which scope applies to each outcome.
- **t1 is the first network action in the plan.** c4 gates live work on authorization; t1 only says "set a budget before external research". give t1 the same explicit authorization gate.
- **c1 is large.** reservations plus lease migration plus index recovery plus five crash boundaries plus two-size benchmarks. keep all of it, but order it: reservations and migration first, index recovery second, and let the benchmark be one synthetic size plus the real archive.

## coherence check

the path is bounded and coherent. c0 pins, c1 makes state safe, c2 replays offline, c3 makes release and review repeatable, c4 proves two supervised cycles, t1 is independent and may end in "no source". raw evidence stays immutable, read-only commands stay read-only, storage caps are set from disk and never satisfied by deletion, blocked sources stay blocked, and trade stays a feasibility question. datacenter usefulness is preserved by reporting exact-model leads separately from row counts and by preferring vendor manuals as the next family.

**intentionally left for c0:** reservation and receipt ordering, restriction granularity, coverage-release shape, overlapping-window identity, ised execution-id handling. all are design decisions the contract is meant to make.

**missing safety semantics, not gates:** cookie redaction on both header paths, and the explicit statement that two-runner tests prove exclusion rather than concurrent writing. those are not choices. write them in before the contract goes to the independent reviewer.

nothing was written, run or fetched. mrrp.
