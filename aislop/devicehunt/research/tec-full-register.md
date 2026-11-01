# Full cached TEC MTCTE register

The original HTML snapshot of [India's TEC equipment table](https://www.tec.gov.in/equipment-certified-under-mtcte)
is now imported in full into `tec_equipment`. This adds 2,331 queryable register
rows independently of the 69 target-model assertions already in the catalog.
It does not establish complete current Indian approval coverage: the cached
table's newest issue date is July 2024.

The new `tec-register` source keeps all twelve source columns, raw cells,
source row number, parsed dates, and date-review status. Applicant (`Company
Name`) and declared OEM (`OEM Name`) remain separate. Primary model and the
additional-model cell are preserved verbatim; no string splitting creates an
alias or a new catalog identity.

Important data quality details:

- 517 rows have no declared OEM; one has no primary model. These remain blank.
- 265 certificate IDs occur more than once. The primary key uses source and row
  number so every row survives, including records for differing variants or dates.
- Ten rows have `18991231 00/01/1900` as their raw expiry cell. The parsed expiry
  is NULL and `parse_status` is `review_required`. Valid issue dates on those
  rows are still parsed. Neither part of the conflicting cell becomes an expiry.
- Dates describe a historical register entry, not current authorization status,
  an operating-system support deadline, or provenance of an auction unit.

The importer validates the table header and complete consecutive row sequence
before replacing its source. The existing target importer shares this row parser
and retains its separate evidence rules. Full-register rows alone cannot pass
the owned-hardware corroboration gate.

## Search and SQL

```sh
python3 -m devicehunt import tec-register data/raw/tec-equipment.html
python3 -m devicehunt lookup 'Omega QMS' --limit -1
python3 -m devicehunt lookup 'AZ4444-52X' --format json
python3 -m devicehunt lookup 'Celestica' --limit 0
```

```sql
SELECT row_number, applicant, declared_oem, primary_model,
       additional_models, certificate_id, issue_date, valid_upto,
       parse_status, source_url
FROM tec_equipment
WHERE primary_model LIKE '%AZ4444%' OR additional_models LIKE '%AZ4444%'
ORDER BY row_number;

SELECT row_number, certificate_id, issue_date_raw, valid_upto_raw, issue
FROM tec_equipment WHERE parse_status = 'review_required';
```

Exports: `data/exports/tec-register.csv` contains all rows;
`data/exports/tec-date-review.csv` contains the ten date exceptions.
`data/exports/lookup-tec-example.json` demonstrates the three AZ4444 variants
with applicant Omega QMS Private Limited and declared OEM Accton Technology
Corporation. None is a bare `AZ4444-52X` identity, so the coverage gap stays open.

Schema version 12 adds the table and indexes. Lookup remains read-only and
reports `not_loaded` on older databases without this table. Rebuild manifests
count the raw register separately from target devices and assertions.
