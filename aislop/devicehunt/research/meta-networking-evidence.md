# Meta networking evidence

Four original public HTML snapshots, captured 2026-09-26, are indexed by
`data/raw/meta-networking-bundle.json`. These are historical platform claims from
the designer/operator. They do not establish an individual auction unit's owner,
current availability, firmware access or software licensing.

| Article | Platform | Target-company relationship | Meta relationship |
| --- | --- | --- | --- |
| [2019-03-14](https://engineering.fb.com/2019/03/14/data-center-engineering/f16-minipack/) | Minipack | Edgecore build partner | Facebook design lineage |
| [2021-11-09](https://engineering.fb.com/2021/11/09/data-center-engineering/ocp-summit-2021/) | Wedge 400, Wedge 400C | Celestica manufacturer | Already deployed at publication |
| [2024-10-15](https://engineering.fb.com/2024/10/15/data-infrastructure/open-future-networking-hardware-ai-ocp-2024-meta/) | Minipack3 | Celestica manufacturer | Announced deployment |
| [2025-10-13](https://engineering.fb.com/2025/10/13/data-infrastructure/ocp-summit-2025-the-open-future-of-networking-hardware-for-ai/) | Minipack3N | Accton manufacturer | Meta design lineage |
| [2025-10-13](https://engineering.fb.com/2025/10/13/data-infrastructure/ocp-summit-2025-the-open-future-of-networking-hardware-for-ai/) | Minipack3 | No manufacturer assertion on this record | Reported fabric role |

The 2019 article describes eight Minipack PIM positions. PIM-16Q operates as
16 x 100G or 8 x 200G, with the other eight ports inactive in 200G mode. PIM-4DD
provides 4 x 400G, and mixed module configurations are described. The article's
software discussion is historical and does not promise a usable image today.

Wedge 400 uses Broadcom Tomahawk 3; Wedge 400C uses Cisco Silicon One. The shared
Wedge prefix is expanded from the article's grouped naming and recorded as such.
Minipack3 uses Tomahawk 5 with 64 OSFP ports and 51.2 Tbps capacity; Minipack3N
uses NVIDIA Spectrum-4 and is also described as a 51.2 Tbps platform.

The later Minipack3 record retains Meta's 2025 account of the platform's role
in building its next-generation fabrics. Its identity namespace is anchored to
the separately validated 2024 manufacturer caption; its own affiliation is only
Meta/documentation_vendor. This neither adds a 2025 Celestica manufacturing
claim nor promotes the wording to explicit completed deployment. See
[the detailed source boundary](minipack3-fabric-role.md).

The 2025 article displays October 13, while its publication metadata is
2025-10-14T00:00:20+00:00. Both are preserved. Deployment relationships are tied
to article dates, not asserted to remain current.

The importer checks canonical URLs, dates, dependency hashes and reviewed text
within article content. Evidence points to numbered paragraph/caption blocks
and normalized-text hashes. Footer text, scripts and image descriptions cannot
substitute for missing evidence. All dependencies are checked before source
replacement. Tests cover altered identities, negation, dates, missing captions,
hash mismatches and repeat imports.

Existing Accton Minipack and the new Edge-Core Minipack retain separate brand
identities. AS8000 is not added as an alias. Minipack2 and other named platforms
without an explicit target-company manufacturing relationship are not imported.
The attempted old Edgecore press URL returned a generic homepage; its downloaded
HTML is excluded from evidence and the rebuild manifest.
