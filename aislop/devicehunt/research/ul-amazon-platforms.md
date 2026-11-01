# Amazon Data Services UL model names

Reviewed 2026-09-27 UTC. The public [UL AZOT7.E515139 listing](https://productiq.ulprospector.com/en/profile/2859718/azot7.e515139?page=75&term=AZOT7)
names Amazon Data Services, Inc. as its listing holder. Web search returned
factual Models entries; direct opening returned HTTP 403 and was not retried.
The cache is a reviewed search extract, not original HTML. Its freshness,
original visual grouping and complete listing coverage are unverified. UL says
its public listing may omit models and entries may describe products or components.

`data/raw/ul-amazon-platforms.json` preserves 74 entries in returned order,
including repetitions. These yield 57 distinct exact names and ten distinct
placeholder templates. The templates remain in the capture and never become
catalog identities or aliases. Examples include `TE1AXYZ`, `Ambrose*****` and
`CMWA#6400-XYZ`; these do not establish concrete products matching the patterns.
The exact name `Tiger_Woods` is separately present and is not derived by stripping
`-xxx`. Full suffixes, underscores and reviewed BARRELEYE names with spaces are
preserved. Original spelling is retained in records; normal catalog identity
matching remains case-insensitive.

The importer adds 57 source assertions and 34 new catalog identities. Examples
of new names include `ArtemisH`, `GrandTetonj`, `CS42_1RP_G2` and `CS48_4LC_E1`.
Existing independent records remain separate. **Listing holder** is the only
company role added; the Amazon Data Services identity namespace does not assert
retail brand, manufacturer, factory, operator, deployment or current validity.
Names shared with Celestica sources are not automatically equivalent hardware.

## Local queries

```sh
python3 -m devicehunt import ul-amazon-platforms data/raw/ul-amazon-platforms.json
python3 -m devicehunt search CS42 --vendor 'Amazon Data Services' --role listing_holder
python3 -m devicehunt lookup AZ9075-64X-DC-42
python3 -m devicehunt vendor-inventory 'Amazon Data Services' --scope all --limit -1
```

```sql
SELECT d.model, a.company, a.role, r.source_url, s.sha256
FROM records r
JOIN devices d ON d.id=r.device_id
JOIN affiliations a ON a.record_id=r.id
JOIN sources s ON s.id=r.source_id
WHERE r.source_id='ul-amazon-platforms'
ORDER BY d.model;
```

Exports: `data/exports/ul-amazon-models.csv`, its JSON counterpart,
`ul-amazon-audit.json`, `lookup-ul-amazon-cs42.json` and
`lookup-ul-amazon-az9075.json`. The manifest includes the hashed factual capture
for offline replay. Tests check placeholder exclusion, exact suffixes and entry
positions, role isolation, idempotent refresh and rejection before mutation.
Your baseline remains 10/10 searchable and 8/10 corroborated, including all four
Oracle models. The complete `7760-32X-A-AC-F` and bare `AZ4444-52X` gaps remain.
