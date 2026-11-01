# query helper and revised promotion pin — accepted, unexecuted

Reviewer `/root/odm_bulk_full`. Accept the reviewed helper code at these exact
SHA-256 values; execution and final integration results remain pending:

- `check_queries.py`: `b670a35b69e0245cc12944bbd6c7485daa8637a6390bfbe68006f779ff699649`.
- revised `promote.py`: `3298b904d0d56fd82bd21a9220d8d893b6702814e48a4a5863c45571b46a356a`.

The checker opens the candidate read-only, checks all 467 source/key records and
5,720 facts, exact raw JSON, source-scoped affiliations/identifiers and absence of
new hyperscaler statements. It compares all manufacturer and hyperscaler rows
against live, records representative specification/buying queries, closes the DB
and requires the candidate file hash unchanged before writing an exclusive
query-check receipt. Its model comparison permits existing catalog case/outer
whitespace normalization; raw candidate strings remain checked exactly in raw
JSON and facts. Query examples are recorded observations, not assertions that
all sample strings must resolve to exact identities or aliases. No blocking code
finding; actual checker output still requires final independent review.

The revised promotion AST is identical to the previously accepted version except
for its required-proof set, which now additionally pins the checker source and
this review. All prior promotion/export findings and recovery limits remain in
[promotion-helper-review.md](promotion-helper-review.md). The export helper is
unchanged at `3942040b7a90b0d7c5beeaac909141f3bf5f1bea185ff37eaab6807694ef00c3`.
[Checkpoint](query-helper-checkpoint.json) and exact source snapshots retain pins.

The user requested graceful shutdown after quota reset. **No staging, query check,
rebuild, promotion or export refresh was executed.** Code/plan acceptance is not
a resumption instruction. No live terminal/tool handles remain; shared state and
all earlier review evidence are unchanged. Stop here pending root's final bounded
documentation check or a later explicit user resumption.
