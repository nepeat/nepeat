# source-scoped PSU facts candidate — 2026-10-12

author `/root/handoff_audit`; outcome: **staged candidate ready for independent fact review**, not imported. only `psu-facts/` written. read the discovery/independent/visual reviews, current curated importer format and role semantics. immutable originals unchanged; no live or scratch imports, network, installs, database/manifest/export writes or delegation.

candidate: `candidate.json`; reproducible builder: `build_candidate.py`; source bindings/hashes: `provenance.json`; completed read-only checks: `checks.json`. six source snapshots are byte-identical copies in `snapshots/`, with original capture timestamps retained by copy2. each source owns exactly one record. SHA-256 of staged candidate: `2c55934a430e423c861f04e107e121ff801e9a36e32985daa032308eab4ef756`.

## all exact rows

| snapshot / source suffix | identity namespace / exact model | cited PDF pages | facts | roles |
| --- | --- | --- | --- | --- |
| bel-pfe1100-series.pdf / bel-pfe1100-series | Bel Fuse Inc. / PFE1100-12-054NA | 1, 2, 5, 9 | 11 | Bel Fuse Inc. documentation_vendor |
| bel-pfe1100-series.html / bel-pfe1100-series-html | Bel Fuse Inc. / PFE1100-12-054NA | HTML JSON-LD exact-SKU object | 2 | Bel Fuse documentation_vendor, spelling retained from HTML |
| bel-pfe1500-series.pdf / bel-pfe1500-series | Bel Fuse Inc. / PFE1500-12-054NAC | 2 | 4 | Bel Fuse Inc. documentation_vendor |
| bel-pfe1500-nemko.pdf / bel-pfe1500-nemko | Bel Fuse Inc. / PFE1500-12-054NAC | 1 | 9 | Bel Fuse Inc. manufacturer; BPS Asia Pacific Electronics (Shenzhen) Co., Ltd. listed_factory |
| bel-pfe1500-csa.pdf / bel-pfe1500-csa | Bel Fuse Inc. / PFE1500-12-054NAC | 4 | 7 | Bel Fuse Inc. documentation_vendor |
| fsp550-29fm.pdf / fsp550-29fm | FSP / FSP550-29FM | 1, plus page-2 conflict only | 9 | FSP documentation_vendor |

every source id is `psu-<source suffix>-2026-10-12`. totals: **six sources / six records / three exact namespace-model identities / 42 facts**. current catalog has no exact identity-model row for any of these three models; strings previously discovered inside other records are not identity aliases. no aliases, additional models, target mappings or family expansions are proposed.

Bel Fuse Inc. is the printed PDF publisher used as a namespace; it does not automatically supply a brand or manufacturer role. HTML separately preserves `brand_as_printed = Bel Fuse`, and contributes exact SKU/series literals only. FSP is its printed logo/vendor namespace. manufacturer appears only on the NAC Nemko record. catalog `vendor_inventory.FACTORY_ROLES` explicitly supports `listed_factory`; the BPS factory stays that separate role and literal fact, not a manufacturer or parent-company alias. all proposed company spellings resolve to no existing target; target scope is not expanded.

## practical statements and boundaries

- PFE1100: preserves 1100 W series class versus 1080 W nominal main output, nominal 12.0 VDC / 90.0 ADC and page-5 general Ta = 0 … +45°C unless noted. normal-airflow standby is 16.5 W at both 3.3/5 V settings. input 90–264 VAC and qualitative voltage/ambient derating are scoped; no current-limitation threshold becomes a continuous rating, no graph points are digitized, and RA's graph is excluded. dimensions remain printed order 321.5 × 54.5 × 40 mm without invented axes. N means normal; no connector-to-connector airflow direction is borrowed from PFE1500.
- NAC datasheet: exact C16 ordering row, 90–264 VAC / 180–350 VDC operating input, normal airflow output connector → input AC socket, upper programmable 3.3/5 V standby group. lower S412 12 V standby is excluded.
- NAC Nemko: dated certificate P22225905/order456320, exact manufacturer/factory/applicant literals, rated AC input 100–240 V ~ / 12–7 A / 50/60 Hz, main 12.0 Vdc / 125 A and standby alternatives. voltage/ambient derating caveat and required system-level compliance checks retained; no current certification or auction-unit provenance claim.
- NAC CSA: page-4 explicitly named regional groups remain separate, with 12.0 V main output and linear derating. Canada/US at 100–200 VAC: 83.4–125 A at45°C,69–104 A at60°C; outside Canada/US:66.5–125 A /66.5–104 A at those temperatures. respective high-input200–240 VAC or200–350 VDC rows are125 A/104 A. other suffix groups' ratings are excluded.
- FSP: page-1 total up to550 W, CRPS2.0, input90–264 VAC/47–63 Hz; output table separately100–240 V input, +12 V max45.8 A and +12 Vsb max2.1 A, no sum of rail maxima. explicit L×W×H185×73.5×39 mm, working0–55°C and PMBus1.2. page-2 FSP550-20FM heading conflict recorded, drawing/airflow excluded. MTBF claims are not part of this bounded candidate.

electrical/cooling/physical statements use the existing categorized fact names where supported. I2C/PMBus are `management_interface_as_printed` literals, not software-installation or entitlement claims. no software installation fact is proposed. each PDF fact includes page citation; HTML facts cite the exact JSON-LD object. full original URLs, source hashes and scope restrictions are in candidate source metadata.

## original source SHA-256

| original snapshot | sha256 |
| --- | --- |
| bel-pfe1100-series.pdf | 245c1e7a503c2f21cedcf1d180fb62623476d9589dfae09a27be38913a95f17e |
| bel-pfe1100-series.html | a2ef4636e4d3c062078621b281f2d3e668b4ed06c0cff13038270439012b4f27 |
| bel-pfe1500-series.pdf | 4a2b9359de8fcbc8ab96b7f25ed78ce289797cf343a5394fdaf5d62a8b719693 |
| bel-pfe1500-nemko.pdf | 2449a20c026afacce8ab7b04e5f48342b5c214680878979dd7feb2d16e4cbfd2 |
| bel-pfe1500-csa.pdf | c7d1e731170257bfbddb3989b43beb49e04dbc95bbddf76cfa9b2acb333fdc60 |
| fsp550-29fm.pdf | 772797d5f26e9de64d129a7de2468fb781ce51be1ef445cdefbd4557d6c36f45 |

checks: all six source ids and snapshots unique, exactly one record/source; all snapshot hashes equal immutable captures and fetch-report entries; all source URLs equal captured URLs; five PDF headers valid; all PDF facts cite pages; no identifier aliases; only Nemko has manufacturer/factory affiliation; no proposed company adds a target. catalog and manifest hashes remain the accepted pair in checks.json. no import execution is claimed.

source QA uses coordinator's `review/psu-fact-pages-review.md` and `review/psu-visual-review.md`. author additionally viewed locally rendered PFE1100 p5, Nemko p1 and FSP p1. first-pass Poppler text is an aid only; superseded extraction strips some default hyphens, so exact model strings are taken from PDFs/HTML and visible review, not normalized extraction strings. complete corrected extraction and independent candidate review remain integration dependencies.

an initial draft accidentally used the common filename stem for the PFE1100 PDF and HTML ids. corrected before handoff: HTML has a distinct `-html` source id; each source now binds its own snapshot. generator assertions prevent duplicate source/snapshot ids and orphan rows.

stop condition met: finite curated candidate and provenance checks ready. next owner reviews exact facts against saved source pages, then sole integrator performs normal preparation/rebuild/checks before any promotion. no chassis fitment, current support, certification validity, marketplace stock, safe standalone operation or custom Murata suffix assertions follow.

final cosmetic revision: removed repeated common source-coverage paragraph from each of six sources. device rows, facts, affiliations and snapshot bindings unchanged. candidate frozen at SHA-256 `2c55934a430e423c861f04e107e121ff801e9a36e32985daa032308eab4ef756`; supersedes reviewed initial candidate `c5f72b43ab6ddfbb6aedf238db533c742848e80b3f150cb5423bd0c86cbc53af` for staging.
