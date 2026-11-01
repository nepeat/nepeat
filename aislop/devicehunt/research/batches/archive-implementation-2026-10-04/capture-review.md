# independent capture review

reviewer: archive_engine (not capture author). outcome: accepted after author fixes.
`python3 -B -m unittest discover -s tests -p 'test_capture.py'`: 17 pass.

independent offline adapter smoke test used a new TemporaryDirectory archive/report
and patched `urllib.request.OpenerDirector.open` to raise if called. adapter
completed all 48 advertised entries, zero HTTP attempt receipts, checkpoint
`completed=48/advertised=48/stopped=false`; archive audit passed. no shared archive,
original capture, catalog, manifest or export was changed. temporary outputs were
removed by TemporaryDirectory cleanup.

independently rehashed every saved success: 41 successful saved-report observations,
40 XML rows plus robots.txt; XML roots parse as sitemapindex/urlset. an initial
probe incorrectly treated robots.txt as XML and failed harmlessly; corrected
format-specific probe passed. these are report-row counts, not distinct-artifact
or historical completeness claims.

fixed findings verified in code/tests:

- network deadline begins before open. built-in transport checks the deadline
  between bounded `read1` operations; final socket wait can extend return to about
  twice supplied timeout. lease must exceed twice timeout, as enforced. collector
  records timeout if elapsed return exceeds nominal timeout. this is a finite
  bound, not a promise of strict interruption at exactly timeout seconds.
- host throttling, URL blocking, saved non-success and successful cache are
  rechecked after lease acquisition; another collector's completed observations
  prevent duplicate sends. owner-aware lease deletion cannot release a replacement.
- adapter validates XML roots/hashes of all advertised successful saved captures
  before archive publication and rejects symlink/traversal paths. saved registration
  additionally checks workspace path safety and expected bytes.
- denied/throttled statuses retain blocked/throttled classification even if final
  URL differs; 304 cannot accept unexpected final URL or lack verified prior bytes.
- saved403/429 without prior Policy objects preserve status classification; null
  errors do not crash policy checks.

request identity includes method, exact URL, body hash and all supplied headers.
latest cache ordering uses frozen receipt timestamps, not deterministic hash IDs.
saved receipt identity includes report hash/ordinal so distinct historical rows
remain addressable; repeating the same registration preserves the wrapper.
HTTP attempts, network304 and local reuse are distinct records. duplicate byte
objects do not manufacture independent corroboration. default transport never
follows redirects; known denials are not retried; unavailable, empty, invalid,
blocked and throttled remain separate states. successful saved observations are
not freshness checks or proof of valid hardware claims.

limits: custom transports must honor bounds; a lease covers bounded acquisition
under normal local scheduling and is not a distributed heartbeat guarantee during
arbitrarily stalled disk/process execution. ordinary receipt/index operation is
local. saved registration currently copies bytes into content objects rather than
registering only the legacy location; this is a storage choice, not provenance
loss, and originals stay unchanged. adapter does not declare worldwide coverage.

next: coordinator may run the finite offline adapter against its selected archive;
no live network budget is authorized by this review.
