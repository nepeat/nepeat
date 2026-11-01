# campaign saved-input shape fix — 2026-10-12

author: `/root/handoff_audit`; outcome: implementation and bounded checks complete, ready for independent review. changed only `devicehunt/campaign_sources.py` and new `tests/test_campaign_input_shape.py`; artifacts only under this directory. original source bytes preserved in `campaign_sources.py.before` with `original-code.json` hash. no network, installs, delegation, importer edits or live archive/catalog/manifest mutations.

the reproduced RRA bundle missing `form` previously escaped as KeyError, leaving the validation job pending. adapters now explicitly reject malformed saved JSON containers/required values with field-specific ValueError messages; the existing runner converts these to checkpointed failed source outcomes. `INPUT_ERRORS` is unchanged: arbitrary KeyError, TypeError, RuntimeError and I/O exceptions still propagate and stop replay with the job pending.

preflight covers concrete known unsafe accesses: RRA form/CSV/detail dependency metadata, query strings, optional page metadata, and the nested company-evidence readers' dictionaries/lists/date strings/composite parts/raw slice values; ISED response dictionaries, path/hash/endpoint/method strings and paged result-name strings. original validators still determine date/query/source/checksum/completeness and company-claim semantics. no output projection, source role, import or gate behavior is added. I/O is not caught. nested company files are read with the existing sibling/hash validator, preserving its path restrictions.

checks (details in `checks.json`):

- eight new tests passed in 7.009s, with malformed required/nested JSON cases, both source adapters' failed checkpoint and reuse behavior, healthy replay/reuse, and programming/I/O exception propagation. network/external process entrypoints are guarded; all writing fixtures live in temporary workspaces.
- 39 affected tests passed in 38.139s: new shape tests plus campaign, RRA and ISED modules.
- independently replayed both adapters read-only against all 12 actual saved pinned units: every output remains `matches_pinned` (`healthy-inputs.json`). this includes composite/historical Celestica evidence. no production campaign run was invoked.
- accepted catalog hash remains `63d3ac788833473f08c716836b528531a4ff7f0df60f270035071fe36243081e`; manifest remains `14c2a1f177051b6e62c8e04dabbd9621a3d4baf6b759d2db25a722ba8efc9195`.

extra read cost: ISED replaces its existing initial JSON read, so adds none. RRA adds one manifest JSON read per unit; the Celestica preflight adds five company-document reads and Accton one. across all 12 current units that is **13 additional reads / 287,166 bytes**, including seven RRA manifests. metadata paths and per-unit sizes/counts are recorded in `healthy-inputs.json`. the preflight does not reread detail pages or CSV bodies. company semantic validators retain their existing reads afterward.

limits: direct importer invocation is unchanged and may still expose malformed shape exceptions; this fix is the assigned campaign adapter boundary only. a full suite/rebuild was not rerun. installed module hashes will change campaign recipe identities, so a future authorized offline campaign will recompute affected jobs rather than reuse results from the old code. live catalog inputs/projections are unchanged, so no catalog import or manifest update is requested.

next: independent engineering review, fixes if needed, then coordinator acceptance/documentation update. no broader write scope needed.

## independent review fix

reviewer found that the shape walker followed unsupported recursive company wrappers before the semantic reader rejected them. fixed traversal to the supported composite → historical → leaf chain; encountering a further known wrapper rejects with ValueError before reading its descendants. regression constructs the reviewer's 1,050-level historical chain, verifies the semantic reader rejects it, and verifies preflight rejects after two reads rather than reaching RecursionError. nested composite bases are also rejected. post-fix new-shape + campaign suite: **28 tests passed**, including nine shape tests, in 35.451s. original healthy source graphs and their read costs are unchanged. ready for re-review.

final re-review follow-up: changed known-wrapper membership from a set to a tuple so ignored list/dict `kind` values cannot raise TypeError. regression preserves semantic-reader handling of those unused fields. final nine shape tests pass; independent reviewer confirms entire outputs equal the original adapter on all 12 saved units, every unit matches pins, and both nesting findings are resolved. `checks.json` now records final code hashes. integrated campaign testing awaits the coordinator's document-runtime stability checkpoint.
