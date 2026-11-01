# IgniteNet MetroLinq manufacturer evidence

Reviewed 2026-09-27. Three catalog records now carry explicit Accton manufacturer
claims, under the documented IgniteNet brand:

| Model | Manufacturer claim | Document |
|---|---|---|
| ML1-60-35 | Accton Technology Corp; Joy Technology (Shen Zhen) Co. Ltd | Sporton FR7D2234-05AA, cover, issued 2018-07-02 |
| ML1-60-19 | Accton Technology Corp; Joy Technology (Shen Zhen) Co. Ltd | Same report and cover |
| ML-60-10G-360 | Accton Technology Corporation | Quick start guide E122017-CS-R01, pages 1 and 5 |

The [Sporton report](https://fcc.report/FCC-ID/HEDML16035/3913123.pdf)
prints two complete model labels separated by a slash. Both remain distinct
identities, with both manufacturer names preserved. The report also names
Accton as applicant; that separate role is not the basis for the manufacturer
claim. Joy Technology has no automatic target-company mapping. A report naming
two manufacturers does not establish which factory made a particular unit.

The [guide](https://fcc.report/FCC-ID/HEDML10G360/3786294.pdf)
names the model on page 1 and the manufacturer on page 5. Its hosting URL
contains an FCC ID, but that URL alone is not imported as a printed-ID assertion.
The ML1 report explicitly prints HEDML16035, which is imported for its two models.

## Capture limitations

These are manually reviewed factual extracts from web-indexed original-document
text on a third-party mirror. They use the `primary_document_mirror` provenance
tier. The cover was available through a search result and the guide through
web-tool PDF text. Direct downloads returned HTTP 403 for both documents;
neither original PDF was archived, visually checked, retried or obtained through
another download endpoint. These extracts are not full searchable PDF captures.
They do not establish current certification, a physical factory, AWS use or
provenance of an auction unit. No electrical or installation advice was imported.

The two small factual snapshots are `data/raw/metrolinq-ml1-report-fields.json`
and `data/raw/metrolinq-ml10g-guide-fields.json`. Source metadata hashes those
snapshots, not the unavailable PDFs. `data/metrolinq-reviewed.json` imports the
three claims through the existing curated importer. All three input files are
hashed in the offline rebuild manifest.

The Nigeria NCC rows for ML-60-10G-360, ML1-60-35 and ML2.5-60-BF-18 were the
research leads. Their abbreviated `Accton Tech. Corporation` manufacturer cell
remains raw and unmapped: this pass adds evidence from different documents,
not a blanket company alias. In particular, it adds no manufacturer claim for
ML2.5-60-BF-18. Existing Accton-namespace regulatory identities remain separate
from IgniteNet-branded identities; two additional brand/model identities do not
mean two newly discovered physical designs.

## Local queries

```sh
python3 -m devicehunt lookup ML1-60-19
python3 -m devicehunt search ML1-60-35 --vendor Accton --role manufacturer
python3 -m devicehunt sql "SELECT d.brand,d.model,a.company,a.role,s.tier,r.source_url FROM records r JOIN devices d ON d.id=r.device_id JOIN affiliations a ON a.record_id=r.id JOIN sources s ON s.id=r.source_id WHERE r.source_id LIKE 'metrolinq-%' ORDER BY d.model,a.role,a.company"
```

Import assertions and source-bound role checks are recorded in
`data/exports/metrolinq-import-audit.json`. Owned hardware remains 10/10
searchable and 9/10 corroborated, with 7760-32X-A-AC-F the remaining gap.
