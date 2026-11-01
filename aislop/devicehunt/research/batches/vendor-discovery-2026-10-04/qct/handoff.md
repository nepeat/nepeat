# qct bounded discovery handoff

outcome: partial named-source research complete; two proposed platform records;
t41s research captured but deliberately excluded from curated import. no shared
catalog/manifest/bundle/export/code/doc writes or tests/imports performed.

## captures and provenance

original bytes, URLs, final URLs, statuses, UTC capture dates, byte lengths and
SHA-256 hashes: `data/raw/vendor-discovery-qct-2026-10-04/fetch-report.json`.
7 source fetches of maximum8; 8 searches of maximum8. stopped without expanding.

- `d52bq-product.html`: original QCT public product, HTTP200;
  sha256 `d5b0fd941c02b83842157ce5e9a4ad8e188fe5abe4bd6e9e07a57fe927061455`.
- `d51b-datasheet.pdf`: vendor-authored QCT-branded two-page document from public
  Art of Server mirror; HTTP200; sha256
  `68418af147f05d4afa468a5fce8a5fad1026168fa90f37b9277b6b8e559ecf7e`.
  original QCT-hosted byte equality is unverified. copyright2014-2017; filename
  `D51B-2U_DS_v03062017.pdf` not selected as product release date.
- `t41s-reseller-datasheet.pdf`: two-page QCT-branded document with HyperScalers
  partner logo; filename suffix `- HS`, copyright2014-2015. sha256
  `4c17cf320cf48f983d049b001c6f8f3161ee33a980428b881f85c8ba15c67ae5`.
  original vendor byte provenance unverified, retained for research only;
  **not proposed as tier-vendor curated source or new reference-PDF import**.
- D51B/T41S guessed original product URLs returned404; preserved status/body.
  public Art of Server index and HyperScalers page retained as link provenance.

## selected supported facts

`proposed-qct-facts.json` uses existing curated format, only QCT brand and
platform_vendor roles. exact full models: QuantaGrid D51B-2U and
QuantaGrid D52BQ-2U. no bare-code alias or Quanta affiliation added.

D51B page2: 2U,447x87.5x745mm; dual E5-2600v3/v4,145W;24DIMMs,
768GB RDIMM/1536GB LRDIMM; alternative24SFF/12LFF front drives plus optional
rear bays;470/500/750/800/1200W redundant AC choices with printed efficiency
labels;4dual-rotor fans,7+1 redundant;IPMI2.0,KVM,QSM/QDCM optional;
5°C-40°C; dedicatedGbE management. input voltage not stated.

D52BQ public Specifications tables:2U,440x87.5x780mm;dual Xeon Scalable,
205W(Refer to CCL);24DIMMs,3TB DDR4 or7.68TB mixed DCPMM+DDR4 claimed;
all five storageSKU tables transcribed with optional rear bay choices;
1+1hot-plug Platinum/Titanium800/1200W PSU; literal six dual-rotor fans,
11+1 redundant;Redfish1.1,IPMI2.0,KVM,QSM1.8 optional;5°C-40°C with
45°C only under unspecified certain situation. voltage not stated. inch/mm
pairs retained without recalculation. page current capacities are separate
from old WSSD configured capacities, neither silently replaces the other.

## excluded / limited

no manufacturers, operators, hyperscaler deployment, current stock, auction
contents, software entitlement or firmware availability asserted. downloads
on D52BQ page show registration/email gate: none requested, and no mirrored
or alternate gated-item acquisition occurred. known Broadwell index403 and
European-office press403 not requested. no firmware/login/contact endpoints.

T41S rendered page2 contains useful **research-only** statements:2U4nodes,
444x87.5x790mm;1600W option200V-240V and1200W option100V-240V;
footnotes1023W at100V-120V~ and1200W at200V-240V~ without conclusively
mapping footnotes to every PSU;fan hot-plug needs cable management arm;
145W andX540 restrictions;16DIMMs. don't promote these until document
provenance is accepted independently. printed heading appends `(4-Node)`;
not automatically an alias of existing bare full model.

## verification / next steps

PDF page2 render visually inspected for both documents; page1 text extracted
for context only. `.pages.json`, `.metadata.json`, PNGs and HTML text extraction
are in this batch directory. Poppler reports missing Japanese font mapping
in contact block; selected English specification tables readable.

reviewer should check hashes, page2 claims against preserved PDF/render and
D52BQ specs against original HTML. integrator may choose accepted2records,
copy D51B PDF byte-identically beside COMPLETE bundle if reference-PDF
capture desired; never replace full bundle with these twoPDFs. curated
snapshot paths already resolve under data/. no new reference bundle supplied.
