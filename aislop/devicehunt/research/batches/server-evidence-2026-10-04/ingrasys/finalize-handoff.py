from pathlib import Path
import json,hashlib,copy
out=Path('research/batches/server-evidence-2026-10-04/ingrasys');raw=Path('data/raw/server-evidence-ingrasys-2026-10-04');old=json.loads(Path('data/ingrasys-facts-2026-10-04.json').read_text());v=json.loads((out/'candidate-curated.json').read_text());corp=copy.deepcopy(next(s for s in old['sources'] if s['id']=='ingrasys-foxconn-portfolio-2026-10-04'));base=next(d for d in old['devices'] if d['source']==corp['id']);corp['id']='ingrasys-recovered-variants-foxconn-portfolio-2026-10-04';corp['coverage']='Explicit cross-source parent portfolio inference for six newly evidenced A/I variants only; not manufacturer. Existing immutable 2026-10-04 Ingrasys company capture plus exact recovered catalog variant pages. Corporate page does not name these models; derivative provenance required.'
recs=[]
for d in v['devices'][:6]:
 r=copy.deepcopy(base);r.update(source=corp['id'],key=d['key'],model=d['model'],title='Ingrasys '+d['model']);f=dict(r['facts']);f.update(model_source_id=v['sources'][0]['id'],model_source_url=v['sources'][0]['url'],model_source_sha256=hashlib.sha256((raw/'server-catalog.response').read_bytes()).hexdigest(),model_derivative_sha256=dict(d['facts'])['source_derivative_sha256'],model_document_page=dict(d['facts'])['document_page']);r['facts']=list(map(list,f.items()));recs.append(r)
(out/'portfolio-proposal.json').write_text(json.dumps({'sources':[corp],'devices':recs},indent=2)+'\n')
(out/'handoff.md').write_text('''# ingrasys finite source handoff

outcome: partial source recovery; eight exact suffixed vendor records proposed. no unsuffixed specification claim is justified. nothing imported.

## evidence and provenance

- original immutable malformed capture retained; fresh one-capture acquisition of same public URL returned byte-identical body SHA-256 `23adb350023b4674ccdac355127ef52961084fe3503f296d56204ebb8818a8b8`.
- fresh body and all HTTP response headers in `data/raw/server-evidence-ingrasys-2026-10-04/`. one web search, one curl invocation; no explicit retry, no Foxconn denied source requested.
- original first PDF span [3,4128651) is an exact prefix of second span [4128654,8355366). first truncated inside content; second lacks final three terminal bytes. no separate version inferred.
- derivative uses second span only and explicitly appends hex `46 0d 0a`, completing `%%EOF` and newline; no object/image/content/xref byte edits. derivative SHA-256 `83cb825a5ff3a087fd2f0706297f640f1b1778fc4ec72d6daf0ac3876465131e`. length matches linearized `/L 4226715`. **this is reconstructed evidence, never a complete valid original download claim.**
- strict pypdf and pdfinfo both resolve ten pages. pdftoppm rendered all ten, all visually reviewed; embedded text blank on every page. see derivative-provenance.json, page-review.json and page PNGs.

## proposed inputs

`candidate-curated.json`: eight vendor records. pp3/4 SV1120A/I; pp5/6 SV1121A/I; pp7/8 SV2121A/I; pp9/10 SV2221A/I. pages 1/2 generic modular overview. facts manually transcribed with raw units, SKU-specific memory/storage/expansion scope and printed `FHAHL` retained. fact-to-page-proof.json binds every fact. source snapshot paths are relative to a planned data/merged.json input.

`portfolio-proposal.json`: separate six new suffix portfolio records using existing immutable about.html parent statement, both original model/corporate URLs and hashes, explicit cross-source inference and derivative/page hashes. no manufacturer role; existing SV2221A/I portfolio records not duplicated.

`reference-pdf-proposal.json`: one derivative document entry, **not a collection**. integrator must copy PDF/text byte-identically beside full preserved COMPLETE bundle, add entry there only after acceptance. no weakening sibling path checks.

## exclusions and conflicts

unsuffixed SV1120/SV1121/SV2121/SV2221 remain index-only. no alias to A/I. current SV2221A HTML describes liquid cooling; recovered page 9 describes air and liquid options, retain as separate dated source assertion. footer years: pages1/2 2022, pages3–7 2023, pages8–10 2025; Oct/2023 index date is not document/release date, PDF metadata modification April2025 is metadata only. SV2121I memory explicitly splits fifth/fourth generation. catalog storage absent on pp7–10, so no storage invented. no AC input voltage/rack power capacity inferred from 48V busbar; no auction configuration/deployment/manufacturer/firmware guarantee.

## checks and next step

original hash and first/second prefix equality verified; strict parser, metadata and all-page visual review pass for **explicit reconstructed derivative**. eight unique vendor names, exact page scopes reviewed. no shared database/manifest/bundle/code/export edits. acquisition stops at 1/8 queries and 1/4 capture invocations because useful page evidence recovered and unsuffixed gap established. independent review must check transcriptions/provenance before integration; if reconstruction is rejected, retain as research only and leave original gap unavailable.
''')
(out/'artifact-hashes.json').write_text(json.dumps({str(p):hashlib.sha256(p.read_bytes()).hexdigest() for folder in [raw,out] for p in folder.iterdir() if p.is_file() and p.name!='artifact-hashes.json'},indent=2)+'\n')
