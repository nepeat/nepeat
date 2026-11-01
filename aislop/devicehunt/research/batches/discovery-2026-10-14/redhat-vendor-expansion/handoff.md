# broad Red Hat vendor expansion

Root authored a finite public-listing collector and deterministic offline parser.
Ten requests (fresh robots + nine literal company queries), all200, no redirects,
retries or detail-page crawl; at least2seconds between queries. Fresh policy
matches accepted prior policy and permits exact public API routes. Source query
shape and companies come from saved accepted primary captures, pinned in plan.json.

All nine current queries are complete:245 rows, each numFound equals captured.
242 explicitly Server/Edge System records retained, three desktop-only records
excluded and preserved in coverage/raw captures. All four ASRock Industrial rows
qualify as Edge System, including one also marked Desktop/Workstation. No cloud,
component or absent-category records promoted. Candidate has9sources/242distinct
identities/242records/3388facts; no exact brand+model or catalog-id hits in live.

Printed partner is the listing-holder identity namespace, not physical product
branding. Exact model field, full product title and independent make field are
preserved. E.g. TYAN-holder MiTAC Tiogapass and Gigabyte-holder FormatServer titles
are not rewritten or aliased. Affiliations are listing_holder only, with no global
target aliases/manufacturer assertions. All9 new vendors remain searchable by
exact printed names; no corporate relationships are inferred. No detailed CPU,
PSU, firmware or support facts are manufactured from listing metadata.

collect.py never parses/imports and stops on unsuccessful response. parse.py
validates hashes, partner/type, unique ids, start/count envelope and exact identity
fields; it retains raw captures and row ordinals. Seven meaningful offline tests
pass, including partner mismatch, duplicate IDs, partial-result flags, title/make
conflicts and category exclusions. prepare_curated passes; repeated isolated
import preserves exactly9sources/242records/3388facts/242listing-holder affiliations,
integrity ok/FK0/no target aliases. Live DB read-only hash unchanged0879a77f….

Candidate/coverage/novelty/validation files are private pending independent review;
no shared production code/catalog/manifest/export changes. Full suite and complete
rebuild are not claimed for this source-author task. Source completeness applies
only to these nine captured queries, not complete vendor histories or worldwide
certification inventory. Root assigns independent review before integration.
