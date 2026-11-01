# Owned-hardware evidence investigation

Reviewed 2026-09-26. The inventory fixture is independent from catalog evidence.
No marketplace availability or AWS deployment is inferred here.

**Current status (2026-09-27): 10/10 searchable and identity-supported.** Nine
matches use collected document snapshots; the tenth uses the user-supplied
[7760 chassis-label photograph](edgecore-7760-label.md), attributed to an eBay
listing. It explicitly pairs `7760-32X-A-AC-F` with supplier SKU `AS7760-32X`.
This resolves the identifier mapping without claiming independent scraping of
a manufacturer document or AWS deployment. The corrected `AZ4444-52X-DC-10`
continues to pass against existing Anatel evidence. Earlier counts below are
historical; no owned identity gap remains under the documented photo-inclusive
validation policy. Automated document-only coverage remains nine of ten.

The user appended Oracle E4-2C, E5-2C, X7-2C, and X10-2C to the baseline.
These entries carry vendor Oracle and no inferred operator. After additional
Oracle/NIST imports, all ten queries return results and seven exact identities
pass in that earlier snapshot. The Broadcom import below now brings this to eight passes; E5-2C has corroboration.
The table below retains the original six-device investigation.

| Owner label | Validation | Evidence / next lead |
|---|---|---|
| CS8200-32X-UN-00 | Hit | UL E362364 extract plus TEC certificate 379401371 naming Celestica Thailand Limited in OEM Name. |
| CS8210-32x-un-00 | Hit | Same UL extract includes the exact UN-00 identifier. NTRA's DC-11 model remains a separate identity. |
| DMC-12-48R-AS0 | Hit | NTRA page 189, row 11955, brand EDGE-CORE. Row 11954's linked-model mention alone would not satisfy the check. |
| 7760-32X-A-AC-F | Reviewed label photo | User-supplied image shows exact Model No. and Supplier Sku AS7760-32X together. Brand and mapping only; photographed-unit fields are scoped to that image. |
| AZ4444-52X-DC-10 (owner-corrected full name) | Hit | Existing Anatel certificate 225812303459 names the exact model and EdgeCore manufacturer. The original shortened label is retained only in correction history. TEC/UL/RRA Accton claims stay separate. |
| AS7772-32X | Hit | NTRA page 189, row 11963, brand EDGE-CORE. |

## Imported source

[Egypt NTRA equipment PDF](https://www.tra.gov.eg/wp-content/uploads/2026/01/Type-Approved-List-3.pdf#page=189)
is cached at `data/raw/ntra-2026-01.pdf` with SHA-256
`72f63661db38c56d1d78fdf97dfe5f8268d5ada85a538c7d6911abe8965bb54a`.
All 760 pages were screened; positional tables on seven candidate pages yielded
119 exact target-brand records. Relevant Edge-Core rows were visually checked.
The English heading says December 2025 while the Arabic heading indicates
January 2026. The catalog does not derive approval dates from that discrepancy.

## UL public-listing extract

- [UL Celestica E362364](https://productiq.ulprospector.com/zh-cn/profile/2858121/azot.e362364?page=23&term=AZOT):
  public web text names the Celestica Shanghai listing holder and both exact UN-00
  models, along with numerous other CS variants. The 25 complete CS identifiers
  were automatically extracted from the returned Models section using the pattern
  `\bCS\d{4}-\d{2}X-(?:UN|DC)-\d{2}\b`, deduplicated and sorted. The saved file
  `data/raw/ul-celestica-models.json` retains the URL, document, holder, retrieval
  time, extraction pattern and identifiers. This is a selected factual extract,
  not original HTML; the direct English-page download returned HTTP 403.
  Importer `ul-capture` records `listing_holder` evidence, not physical OEM or
  AWS use. No account or access restriction was bypassed. These source fields
  were extracted from the retrieved listing, not copied from the inventory fixture.

## Additional imported evidence

- [TEC equipment table](https://www.tec.gov.in/equipment-certified-under-mtcte),
  cached in `data/raw/tec-equipment.html`: 2,331 certificate rows, 42 matching
  target rows, 69 separately named model assertions. Its newest displayed issue
  date is July 2024. CS8200-32X-UN-00 has certificate ID 379401371; the three
  AZ4444 variants use 379402315, 379401544, and 379402317. Company Name identifies
  the applicant, while OEM Name provides the declared OEM. These fields do not
  establish AWS deployment. Raw dates are retained in the source record and
  parsed dates remain historical register assertions.
- [eCrunch complete 7760 ordering code](https://www.ecrunch.com/edge-core-7760-32x-a-ac-f-as7760-32-100g-ethernet-data-center-spine-switch-red-i610661.html),
  cached in `data/raw/ecrunch-7760.html`: exact identifier extracted from the
  product heading, seller SKU I610661 checked. Only seller-reported identity is
  imported; prices, availability, specifications and previous ownership are not.
  It produces a local search result but does not satisfy the corroboration gate.

All six queries now have results; four have corroborated exact identities.

## Leads still outside the imported evidence

- [UL Accton E141136](https://productiq.ulprospector.com/en/profile/2858881/azot7.e141136?term=e141136):
  public web text includes AS7772-32X, DMC variants, and AZ4444-52X-DC-10,
  AZ4444-52X-DC-42, AZ4444-52X-AC-00. Direct download returned HTTP 403.
- [ICASA equipment PDF](https://www.icasa.org.za/uploads/files/List-of-Type-Approved-Equipment-2025.pdf):
  now imported as a separate register; see the ICASA section below. The PDF has
  no brand column, and suffixed AZ4444 rows do not resolve the bare label.
- [eBay item 389108536363](https://www.ebay.com/itm/389108536363):
  web retrieval has the complete 7760 code in Model and MPN fields. Additional
  seller corroboration, not imported as manufacturer evidence or a live offer.

An AS7772 device-tree search also surfaced a gist under `nepeat`. It was not used
to close an independent coverage check. The NTRA row is sufficient without it.

Next: investigate the full 7760 code and bare AZ4444 label. Original-document
copies and additional TEC/RRA corroboration would strengthen the Celestica entries.


## Oracle baseline evidence

| Baseline label | Validation | Source and scope |
|---|---|---|
| E4-2C | Hit | [NIST CMVP 4506](https://csrc.nist.gov/projects/cryptographic-module-validation-program/certificate/4506), tested software configuration names Oracle E4-2C Server. Not server hardware certification. |
| E5-2C | Hit | [Broadcom server-guide record 57328](https://compatibilityguide.broadcom.com/detail?program=server&productId=57328&persona=live): original cached public search and detail JSON agree on Oracle Server E5-2c and Oracle partner. The older Geekbench submission remains a separate weaker lead. |
| X7-2C | Hit | [Oracle x86 source-code index](https://www.oracle.com/downloads/opensource/x86-servers-source-code-downloads.html), explicit Oracle Server X7-2C heading and archive links. |
| X10-2C | Hit | [Oracle licensing documentation](https://docs.oracle.com/en/engineered-systems/private-cloud-appliance/3.0-latest/licensing-3.0-latest/licensing-component-x10-2c-license.html), explicit hardware component statement. |

The Oracle and NIST sources have cached original HTML and source hashes;
Broadcom has cached original API JSON. The
source-code index contains E5-2L in an Exadata heading; that is a different
suffix and never becomes an E5-2C alias. Archive files were not downloaded.

An indexed VMware systems guide also mentions E5-2c. Its former PDF URL now
redirects to Broadcom's compatibility web application; the downloaded content
is HTML, saved as `data/raw/vmware-systems-redirect.html`, and was not imported
as a PDF or treated as independently verified compatibility evidence.


## ICASA snapshot now imported as separate register rows

The full 906-page PDF is cached and its 26,258 rows are searchable with `registry`.
There is no manufacturer/applicant column. No manufacturer relationship or
baseline pass is created from a model-string match alone. Identified references:

| Model field | PDF page | Reference |
|---|---|---|
| DMC-12-48R-AS0 | 153 | TE-2025/0019 |
| DMC-12-48R-AS0 48V | 153 | TE-2025/0183 |
| AZ4444-52X-DC-10 | 224 | TE-2024/0223 |
| AZ4444-52X-DC-42 | 225 | TE-2024/0297 |
| AZ4444-52X-AC-00 | 227 | TE-2025/0177 |

The DMC 48V label and each AZ4444 suffix stay intact. Page 735 also contains a
shifted row mentioning CS8210-32X-DC-11 / CS8211-32X-DC-11: its interpreted fields
are NULL and raw cells are retained for review. Pages 224 and 906 were visually
checked to confirm both normal columns and the source's shifted-column issue.
This source strengthens discovery leads but leaves the three identity gaps open.


## Power / airflow follow-up

An indexed manufacturer quick-start guide for AS7710-32X, AS7760-32X and
AS7761-32X was located in the public ANATEL mirror. Downloading the original
PDF returned HTTP 403. Its indexed content does not establish equivalence to
7760-32X-A-AC-F, so no new alias or baseline pass was created.

For other Edgecore products, the cached official Features sections now provide
buying-relevant statements with source links. DCS204 / AS7726-32X, for example,
has qualified AC-or-48V-DC PSU, airflow-SKU and ONIE statements. Those are not
transferred to AS7760, AZ4444, or other superficially similar models. See the
`specs` command; `--exact-model` excludes compatibility-only mentions.


## Broadcom corroboration closes E5-2C

The new public compatibility API yields a complete 24-record Oracle server query.
Original search/detail responses are cached with checksums and repeat-query
consistency checks; see [source review](broadcom-server-evidence.md). E5-2C now
passes, while E4-2C and X10-2C gain additional corroboration. Baseline is **10/10
searchable and 8/10 corroborated**. The remaining gaps are 7760-32X-A-AC-F
(seller-only complete code) and AZ4444-52X (variants without sufficient bare-model
and expected-vendor evidence). No validation criterion was relaxed.


## Anatel variant evidence

The official bulk register now corroborates EdgeCore manufacture claims for the
three complete AZ4444 variants (-AC-00, -DC-10, -DC-42). It also lists AS7760-32X
under EdgeCore. Neither establishes the bare AZ4444-52X label or the complete
7760-32X-A-AC-F ordering code, so both baseline gaps remain open. Additional
exact evidence for CS8200-32X-UN-00, DMC-12-48R-AS0 and AS7772-32X is imported.
See [Anatel review](anatel-product-register.md).

## Public UL Accton platform list

The abridged AZOT7.E141136 Models section is now cached as a factual web-tool
extract; 191 reviewed platform identifiers are imported with listing-holder
evidence. It includes AS7760-32X and all three complete AZ4444 variants, but
neither missing baseline label. No suffix was stripped to create a pass.
See [UL source review](ul-accton-platforms.md).

## Further exact-label search and full TEC lookup

The follow-up exact-label searches still found seller evidence for the complete
7760 ordering code and suffixed AZ4444 variants. The public UL
[BR4384 listing](https://productiq.ulprospector.com/en/profile/5829236/anatel%20-%20brbw.br4384?page=2&term=BRBW)
names Openglobe Tecnologia Eireli as listing holder and the three complete
AZ4444 variants in its Models section. Its shortened summary ends in an ellipsis;
that truncation is not evidence for the bare baseline label. The full section
does not resolve either gap. This page was reviewed as a lead, not imported as
new target-company or bare-model evidence.

The entire 2,331-row cached TEC table is now available through local `lookup`
and SQL, including applicants outside the five target organizations. The three
AZ4444 rows retain Omega QMS as applicant and Accton as declared OEM. Their
complete suffixes remain intact. See [full register](tec-full-register.md).

## Follow-up search alongside Panorama expansion

Another exact-label pass still found the complete 7760 ordering code on seller
pages and AZ4444 with full suffixes in certification sources. Public trade-data
snippets also mention AZ4444-52X-MAIN and AZ4444-52X-CPU components; those are
different identifiers, not evidence for a bare chassis label. No new alias or
baseline pass was created. Broader Amazon manufacturer research identified
AMHT-1 in an original AWS declaration; that separate addition is documented in
[AWS Panorama evidence](aws-panorama.md).


## December 2022 historical NTRA review

The complete older register is now archived as 29,208 separate snapshot rows,
including AS7760-32X on page 460. Its combined BRAND & MODEL column remains
unsplit. It does not close either exact-label gap. Historical comparison and a
new Oracle adapter discovery lead are documented in
[ntra-historical-register.md](ntra-historical-register.md).

## Further exact-label search after Taiwan imports

The complete 7760 ordering-code search, a reduced `7760`/`A-AC-F` query, official
Edgecore/GitHub AZ4444 queries and Korean/RRA-targeted queries did not establish a
new independent exact-label source. Returned seller entries still do not satisfy
the gate; missing search results are not proof that no documentation exists.
Both gaps remain open. Separate follow-up of the new Oracle registry codes found
original EcoDesign manufacturer documents; see [Oracle evidence](oracle-ecodesign.md).

## Follow-up after the five official Korean queries

The complete RRA captures add original Accton evidence for AS7760-32X and three
suffixed AZ4444 variants. Neither is the missing expected Edge-Core identity.
A further full-code search excluding the previously seen seller domains found
another marketplace listing, still seller evidence; an official Edgecore-domain
AZ4444 query did not establish a new source. Both gaps remain open. The separate
[Celestica company-name review](rra-celestica-company-names.md) improves broader
manufacturer coverage without changing either validation expectation.

## PICOS follow-up — 2026-09-27 UTC

The newly captured original PICOS 4.5 Edgecore/Accton hardware table contains
26 platform rows; neither missing owned label is present. Exact-code searches
again surfaced seller descriptions and suffixed AZ4444 regulatory entries.
No baseline alias or manufacturer claim was inferred. The table does add
independent software-vendor specifications; see [scope and local queries](picos-hardware.md).

September 27 follow-up: renewed exact 7760 ordering-code and AZ4444 searches
still surfaced seller-only full codes, suffixed regulatory models and separate
board components. They did not close either gap. The newly collected official
Nigeria NCC register has neither missing exact label; all 8,517 published rows
are now retained for independent local searching. See
`research/nigeria-approved-equipment.md`. No baseline criteria were relaxed.

Ghana NCA follow-up: the complete returned public table has 1,153 rows and
neither missing exact Edge-Core label. The sole exact target manufacturer row
is Oracle America Inc. for the fully printed `Oracle's Netra Sparc T3-1` label.
Two original table downloads agree; the linked Oracle detail page returned
HTTP 500. Imported table evidence does not imply a retrieved certificate or
current/historical completeness. See `research/ghana-approved-equipment.md`.

## Partial-model discovery follow-up

The local `search 7760` command previously missed both AS7760-32X identities
because FTS only matched word prefixes. Model/identity fragment matching now
returns them alongside the seller's full ordering code. This is a search repair,
not corroboration of equivalence: both baseline gaps remain unchanged. See
`docs/model-fragment-search.md`.

A newly located Master MT-3593/2015 certificate mirror names AS7710-32X and
AS7760-32X. The web-tool PDF text was reviewed before its direct-byte download
returned HTTP 403. The denied URL was not retried or accessed by another route;
no original PDF or catalog assertion was imported. See
`research/as7760-certificate-lead.md` for the preserved lead and limitations.
