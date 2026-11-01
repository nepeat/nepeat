# accessory revision listing leads

Outcome: accepted bounded code milestone, 2026-10-12. Coordinator `/root` authored;
independent reviewer `/root/handoff_audit` [accepted](../review/listing-revisions-review.md).
No catalog or manifest writes, imports, network requests or new identity aliases.

Buyer audit F13: bare `YM-2651Y` previously missed the four printed PSU revision
identities. It now returns four variant leads. `YM-2651Y-B01R` returns only the
two B01R spellings. Exact stored strings retain their normal complete mentions.
The extra rule applies only to model fields of `Unattributed accessory` identities
whose kind contains the word FRU/FRUs, and requires a hyphenated alphanumeric base
plus a separate short alphanumeric revision. Every revision stays distinct.

Files: `devicehunt/listings.py`, `tests/test_listing_accessory_revisions.py`,
`docs/listing-matching.md`. Extra matches retain source-record scope and receive
no specifications; aliases, server identities and unrelated facts do not gain
the rule. Added hyphen/plus/underscore extensions cannot become complete matches.

Checks: [34 focused tests](checks.json) pass; independent reviewer reproduced the
pass and checked a multi-record vendor/seller fixture. Existing tests emit SQLite
ResourceWarnings but no failures. [Ten live title probes](live-smoke.json) cover
the PSU spellings, unknown revisions and several owned models. Database SHA-256
before/after is `63d3ac788833473f08c716836b528531a4ff7f0df60f270035071fe36243081e`.
This is focused listing verification, not a new full-suite or rebuild result.

Limits: a spaced bare base can still lead to other revisions when a title also
contains a separated revision. These are explicitly weaker family leads. No
component compatibility, electrical rating or item-for-sale identity is inferred.
The broader open-ended search goal remains active until the user's cutoff.
