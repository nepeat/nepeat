# bounded capture handoff — 2026-10-04

owner: archive_capture. only capture.py, test_capture.py, the sitemap adapter and
this note were edited. no network calls, installs, catalog writes or shared data
mutations. temporary offline registration was under /tmp.

`Collector(archive, Policy(allowed_urls, budget=0, max_bytes=12000000,
timeout=30, lease_seconds=120, prior_failures=()), request_transport=transport,
clock=time.time)` provides `capture(url, method='GET', body=b'', headers=None,
refresh=False, expected=None, validator=None)` and
`register_saved(row, path=None, locator=None)`. Archive contracts are receipt,
receipts, resolve and store_bytes. receipt kinds capture_attempt, capture_result,
capture_reuse and capture_policy separate acquisition from reuse and policy.

request identities preserve exact URL, uppercase method, body hash and all
supplied headers. verified cache reuse has no new network acquisition time.
refresh validators are wire headers and do not alter representation identity.
304 needs a verified prior capture. newest captures are ordered by receipt
created_at, not UUID filename. saved receipts have deterministic IDs including
original row, bytes and optional report/ordinal locator; unknown historical
request context is explicitly marked. duplicated rows can remain distinct.

request/host leases use short SQLite transactions in capture-leases.sqlite,
released in finally; expired leases can be recovered and former owners cannot
release a replacement. no transaction spans transport. the default urllib
transport does not follow redirects, uses read1 in bounded chunks with a total
deadline into a unique temporary spool, then publishes retained bytes through
archive.store_bytes. lease TTL must exceed twice timeout to cover deadline plus
one bounded socket wait. injected transports must honor the supplied bounds;
elapsed return time and byte length are also checked. partial oversized bodies
are retained with body_truncated=true, not successful artifacts.

402/403 policy evidence persists across collector instances and cannot be
refreshed or bypassed via automatic redirects. stored unsuccessful results and
saved failures are reused without default retries. 429/503 checkpoint the host;
new collector instances also stop on that saved throttle. errors, empty,
unavailable, invalid, blocked and timeout are distinct. HTML cannot be acquired
or reused as PDF evidence; caller validators apply to cached representations too.

finite offline second-format invocation (coordinator owns actual paths):

```sh
python3 research/fetch_fcc_sitemaps.py --archive <archive-root> --report-dir <new-reports-dir>
```

network budget defaults to zero. the adapter validates all saved hashes before
registration, imports every original report row with report hash and ordinal,
uses only the 48 advertised exact child URLs, and writes a new UUID-versioned
report without touching old captures/reports. offline smoke test observed 48/48
saved children: 38 success and 10 unavailable. original two denied alternate
hosts are retained as saved policy/capture failures without requests. coverage is
finite advertisement coverage, never complete FCC or device evidence.

verification: `python3 -m unittest tests.test_capture` — 17 tests pass. checks
cover contention/recovered leases/interruption, exact identities, changed URL
versions, two URLs sharing bytes, cache checksum rejection, cache type checks,
304, deterministic saved IDs with duplicate ordinals, persisted failure reuse,
denial/error-body retention, throttling, size/timeouts and redirects. adapter
smoke invocation used /tmp/fcc-capture-offline-check-v3 and saved inputs read-only.

independent review corrections: total transport deadline begins before open;
normal socket operations can run through one final wait up to twice timeout,
while collector classifies elapsed >timeout as timeout. lease exceeds this bound.
after lease acquisition, saved errors and throttles are checked again. status
policy applies even when a denied/throttled response has an unexpected final URL;
304 redirects are invalid. saved paths reject symlinks and child XML roots are
validated before any archive writes. deterministic historical receipts preserve
all source-report duplicate ordinals. invalid saved XML pre-write rejection is
covered by the adapter test. combined engine/capture suite: 30 tests pass.
