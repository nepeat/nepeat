# Accton platform identifiers in UL's public listing

Reviewed 2026-09-26. The source is the public abridged
[AZOT7.E141136 listing](https://productiq.ulprospector.com/en/profile/2858881/azot7.e141136?term=e141136),
held by **ACCTON TECHNOLOGY CORPORATION**, in the category for audio/video,
information and communication technology equipment certified for Canada.

The web retrieval exposes a flattened Models section. Its factual model text,
document identifier, holder, URL and retrieval timestamp are cached in
`data/raw/ul-accton-platforms.json`. This is a structured web-tool extract, not
original HTML. The public page explicitly says that it does **not** show every
model in the listing. No login-only material is used.

## Extraction and evidence scope

`ul-platforms` now extracts **437 distinct identifiers**. The original capture's
191-name extraction covers AS/AZ/AIS/ECS/DMC/CSX/AMX/Wedge naming families,
Minipack, Barefoot DNA X1032C and ORV3 HSM. The importer still recomputes that
original list and rejects a mismatch before applying the expanded extraction.
An additional reviewed rule covers 34 complete QFX/EX/ACX/JNP/SRX/JCP identifiers;
a further rule covers 212 complete commercial codes and named models.
The original capture and its hash are unchanged; this is an expansion of local
parsing, not a new or more recent UL retrieval. Each assertion records its
extraction scope and exact character offsets into the same cached section.
Numeric-only names, ambiguous 1FINITY groupings, other unreviewed names and
wildcard families remain unparsed, so 437 is an
extraction count, not the listing's total model count.

Whitespace qualifiers stay attached: `AS9516-32D (16-Core)`,
`DMC-12-48R-AS0 48V`, `ECS4100-12T v2`, `ECS4100-28TC-F US2`, and
`AS5915-16X AC` are not reduced to shorter identifiers. An unreviewed parenthesis,
voltage, version or regional qualifier stops extraction for review. Hyphenated
suffixes remain complete, including all six AZ4444/AZ4445 AC/DC variants. Bare
forms are included only where the section independently lists them.

All records use the **listing_holder** relationship. This does not establish
physical manufacture, equivalence with another brand's model, hyperscaler
deployment, firmware availability, certification validity today or the previous
owner of a surplus unit. The Accton identity namespace is used consistently with
other registry imports; it does not assert Accton as the product's retail brand.
Each record now carries that namespace limitation as a searchable fact. Inspect
affiliation roles when querying manufacturers.

## Commercial identifiers

The expansion includes ten EX4100 variants, eight QFX identifiers, nine ACX
identifiers, four SRX identifiers, two JNP7100 identifiers and JCP-JNET2-ID.
For example, `EX4100-48T` and `EX4100-F-48T` remain distinct, as do `ACX7024`
and `ACX7024X`. The listing does not make `JNP7100-32C` an alias of
`ACX7100-32C`; both are retained independently. Wildcards are not expanded,
embedded fragments of longer tokens are not imported, and unreviewed spaced
qualifiers stop the import before it replaces any existing evidence.

Juniper's public [EX4100 system overview](https://www.juniper.net/documentation/us/en/hardware/ex4100/topics/topic-map/ex4100-system-overview.html)
and [QFX5241 documentation index](https://www.juniper.net/documentation/product/us/en/qfx5241/)
provide brand research leads. Those pages have not been imported as evidence:
the UL records retain only their actual listing-holder relationship. A separate
search found an EX4100 factory-certificate copy on a document-sharing site, but
no publicly accessible primary certificate was obtained. The Juniper compliance
portal requests login; no gated materials were retrieved or inferred.

```sh
python3 -m devicehunt search EX4100 --vendor Accton --role listing_holder
python3 -m devicehunt match-listing --title 'Juniper EX4100-48T switch' --vendor Accton
```

An equivalent SQL query for the newly parsed section is:

```sql
SELECT d.model, a.company, a.role, r.source_url,
       json_extract(r.raw_json, '$.model_section_spans') AS section_offsets
FROM records r
JOIN devices d ON d.id = r.device_id
JOIN affiliations a ON a.record_id = r.id
WHERE r.source_id = 'ul-accton-platforms'
  AND json_extract(r.raw_json, '$.extraction_scope') =
      'qfx-ex-acx-jnp-srx-jcp-identifiers-v1'
ORDER BY d.model;
```

## Findings and unresolved labels

The additional 212-name extraction covers JL/R/S commercial part codes,
complete FortiSwitch-AX/FS-AX/AX names, GigaVUE-TA identifiers, Vision variants
and selected other concrete model families. Examples include `S1A527A` and
`S1A527AB`, retained separately, and `Vision E100 AC` versus `Vision E100 DC`.
Similar strings such as `FS-AX2340S-48P4X`, `FortiSwitch-AX2340S-48P4X` and
`AX2340S-48P4X` are separate identifiers, not inferred aliases. No retail-brand
or manufacturer relationship is added from their prefixes.

58 names have adjacent RMN annotations. Those are retained verbatim in raw
records and as `ul_adjacent_annotation` facts. The flattened capture does not
establish the grouping of every comma-separated list: in
`JL675A, R8N85A (RMN ID:RSVLC-1810)`, the annotation is retained with the
immediately preceding R8N85A token only, without assigning it across the list.
It is not imported as an alias, identity or proof of product equivalence.
Unknown parenthetical, voltage, version or regional qualifiers stop the import
before replacing any source records. Templates containing wildcard sequences
remain excluded; no concrete models are synthesized from them.

```sh
python3 -m devicehunt search GigaVUE --vendor Accton --role listing_holder
python3 -m devicehunt search RSVLC-1810 --vendor Accton
python3 -m devicehunt match-listing --title 'Surplus JL680A switch' --vendor Accton
```

The latest expansion adds 212 source assertions and 201 catalog identities.
It reuses the original unchanged capture and timestamp; no fresh UL request was
made. Its extraction scope is `additional-complete-commercial-identifiers-v1`.
The separate `data/exports/ul-accton-extended.json` and extraction-audit JSON
record this scope and the still-unparsed text. The audit is not device evidence.

The selected models include Wedge100BF-32X/-32QS/-65X, Minipack and
Minipack-AS8000, AIS800-32D/-32O/-64D/-64O, CSX4500/8300/8500, AS9936-128D/-128DV,
and ORV3 HSM. These are searchable identification leads with listing-holder
evidence, without inferred operator relationships.

The source lists AS7760-32X and the complete AZ4444-52X-AC-00/-DC-10/-DC-42
variants. It does not establish the owner's complete `7760-32X-A-AC-F` ordering
code or a bare `AZ4444-52X` identity. Both baseline gaps remain open. Other
searches in this pass found seller copies of the 7760 code and additional
suffixed AZ4444 references; none supplied the missing primary equivalence.

```sh
python3 -m devicehunt import ul-platforms data/raw/ul-accton-platforms.json
python3 -m devicehunt search 'ORV3 HSM' --vendor Accton
python3 -m devicehunt search Wedge100BF --vendor Accton --role listing_holder
```

The initial import added 191 assertions and 118 catalog identities; the expanded
parser adds another 34 assertions and 34 identities. It is included in the
offline rebuild manifest. All 437 source records are exported in
`data/exports/ul-accton-platforms.json`; the earlier 34-name expansion remains in
`data/exports/ul-accton-commercial.json` and the latest 212-name expansion is in
`data/exports/ul-accton-extended.json`.
