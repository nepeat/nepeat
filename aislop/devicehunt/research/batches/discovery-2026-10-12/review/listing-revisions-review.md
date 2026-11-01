# accessory revision listing review — 2026-10-12

reviewer: `/root/handoff_audit`; outcome: **accept**. no blocking findings. reviewed `devicehunt/listings.py`, `tests/test_listing_accessory_revisions.py`, `docs/listing-matching.md` and the saved `listing-revisions/live-smoke.json`. only this review was written; no catalog/import changes or network.

the added rule is narrow: exact brand `Unattributed accessory`, a whole FRU/FRUs word in the identity's kind, the matching record's model field, and a hyphenated base containing letters and digits with a separate 1–8 alphanumeric revision. server models and alias identifiers sharing the same text do not gain the new lead behavior. existing generic matching remains in force for them.

the bare code returns distinct revision identities as `variant_lead`; a hyphenated revision returns only the matching printed revision. ordinary exact spelling retains the existing `model_mention` path. Unicode dash normalization applies before matching. underscore, plus, extra letter and additional hyphen suffix boundaries reject the extended code instead of lending an exact identity. variant leads have no specifications; existing exact matches retain statements from their own matching record.

checks run:

- independently ran the four focused modules (`test_listing_accessory_revisions`, `test_listings`, `test_listing_software_refs`, `test_listing_specifications`): 34 tests passed in 25.309s. ResourceWarnings about unclosed SQLite fixture connections appeared; they did not cause failures and no connection-lifecycle code is added by this change.
- additional temporary multi-record fixture: one FRU identity had Edge-Core vendor evidence and separate Oracle seller evidence. filtering for Oracle returned only the seller record; filtering for Edge-Core returned only vendor records. both remained variant leads with empty specifications. this checks that a shared identity does not lend another record's vendor or stronger tier.
- additional unknown suffix probes: `YM-2651Y-B01R-C99R`, `YM-2651Y+B01R`, and `YM-2651Y-B01R_` returned no mention. a slash-separated revision can leave a bare-code lead, consistent with punctuation terminating the base token; no equivalence or fitment is asserted.
- inspected saved live smoke: bare `YM-2651Y` returns four distinct B01R/C01R identities; hyphenated B01R returns its two separate spelling identities; unknown C99R returns zero. exact 7760, AZ4444, AS7772 and E5-2C behavior remains useful. all variant specification lists are empty.
- live catalog hash after independent review remains `63d3ac788833473f08c716836b528531a4ff7f0df60f270035071fe36243081e`, equal to the smoke's before/after hashes.

documentation describes the extra matching rule, separate identities, suffix restriction, record scope, and no-fitment boundary accurately. no new source/evidence claims are introduced. this is a local matching change; accepted manifest and catalog counts need no integration.
