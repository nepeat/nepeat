# Minipack3: the 2024 plan and 2025 fabric-role account

The [2024 Meta article](https://engineering.fb.com/2024/10/15/data-infrastructure/open-future-networking-hardware-ai-ocp-2024-meta/)
identifies Celestica as Minipack3's manufacturer and announces intended
deployment. That existing record remains unchanged.

The [October 2025 article](https://engineering.fb.com/2025/10/13/data-infrastructure/ocp-summit-2025-the-open-future-of-networking-hardware-for-ai/)
describes Minipack3 and Cisco 8501 as a foundation used in building Meta's
next-generation frontend and backend data-center fabrics. The original article
HTML was already in the checked Meta source bundle; the relevant paragraph was
reviewed again against the public article on 2026-09-27.

This adds `documented_fabric_role` for the existing Minipack3 identity, separate
from the 2024 `announced_platform_deployment` assertion. The wording does not
explicitly state completed deployment, fleet size, current use or a particular
unit's history, so it is not counted as `documented_platform_deployment`.

## Source and company boundaries

The 2025 paragraph does not name Celestica. Its record therefore has only
Meta/documentation_vendor affiliation. The Celestica identity namespace comes
from the separately validated 2024 Minipack3 caption; its source ID, URL and
HTML hash are recorded as namespace evidence. Both articles must validate
before an import can replace any Meta source.

This distinction matters for querying: the new assertion is visible in the
global hyperscaler report and in the model's evidence. The strict Celestica
vendor-filtered hyperscaler report excludes it because that particular record
has no Celestica affiliation. The 2024 Celestica manufacturer record is retained.
Minipack3N remains a distinct Accton-manufactured identity and receives none of
Minipack3's fabric-role or deployment claims. Cisco 8501 is not added to the
target-company catalog by association with the shared paragraph.

The displayed date is 2025-10-13; publication metadata is
2025-10-14T00:00:20+00:00. Both remain source dates, not deployment dates.
The exact model context and role wording must occur together in one article
paragraph. Tests reject altered models, future-tense wording, negation and
invalid manufacturer evidence in the separate namespace source before mutation.

```sh
python3 -m devicehunt hyperscaler-inventory --relationship documented_fabric_role
python3 -m devicehunt lookup Minipack3
```

```sql
SELECT d.model, r.source_id, h.relationship, h.detail
FROM devices d JOIN records r ON r.device_id=d.id
JOIN hyperscaler_evidence h ON h.record_id=r.id
WHERE d.model='Minipack3'
ORDER BY r.source_id;
```

The complete 7760-32X-A-AC-F code was also searched again. Results still provided
seller descriptions rather than independent complete-code corroboration. The
owned baseline therefore remains 9/10 corroborated; no AS7760 family alias or
relaxation of the validation rules was added.
