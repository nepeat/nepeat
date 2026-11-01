"""Prepare a finite curated candidate from immutable captures; never import it."""
import hashlib
import json
from pathlib import Path
import shutil

HERE = Path(__file__).resolve().parent
CAPTURES = HERE.parent / 'psu'
LIMITS = ('Source-scoped exact model statements only; no chassis fitment, model equivalence, current '
          'certification/support, safe wiring/installation recommendation or individual auction-unit '
          'configuration/provenance. Identity namespace alone is not brand/manufacturer evidence. '
          'Other printed models and suffixes are excluded; conflicts and conditions are not reconciled.')
BEL_NAMESPACE = ('Bel Fuse Inc. is the printed PDF publisher used as an identity namespace, '
                 'not a brand/manufacturer inference. Manufacturer and factory claims appear only '
                 'on the separately scoped Nemko record.')


def fact(name, text, page):
    return [name, text + ' [PDF p. ' + str(page) + ']']


def source_id(file):
    return 'psu-' + Path(file).stem + ('-html' if Path(file).suffix == '.html' else '') + '-2026-10-12'


def row(file, model, brand, facts, pages, affiliations=None, limits=None):
    return {'source': source_id(file), 'key': model,
            'brand': brand, 'model': model, 'title': model + ' power supply module',
            'kind': 'power supply module', 'affiliations': affiliations or [[brand, 'documentation_vendor']],
            'identifiers': [], 'facts': facts, 'reviewed_pdf_pages': pages,
            'scope_limitations': LIMITS + ' ' + (limits or '')}


devices = [
    row('bel-pfe1100-series.pdf', 'PFE1100-12-054NA', 'Bel Fuse Inc.', [
        fact('model_evidence', 'Figure 20 explicitly names PFE1100-12-054NA; series heading is PFE1100-12-054xA.', 9),
        fact('power_description', 'PFE1100-12-054xA series is described as a 1100 Watt AC to DC power supply with 12 VDC main output; this class description is distinct from the nominal main-output table rating.', 1),
        fact('power_description', 'Series wide input voltage range: 90–264 VAC; output derating applies at low input voltage/raised ambient temperature.', '1, 9'),
        fact('power_description', 'Main output V1 nominal voltage 12.0 VDC; nominal output power 1080 W and nominal output current 90.0 ADC at V1 = 12 VDC. Table general condition Ta = 0 … +45 °C unless otherwise noted; these are not current-limitation thresholds.', 5),
        fact('power_description', 'Always-On 16.5 W programmable standby output (3.3/5 V) is a series feature; it is not an instruction to sum rail maxima.', 1),
        fact('power_description', 'Normal-airflow standby nominal ratings: 3.3 VDC, 16.5 W, 5 ADC; or 5.0 VDC, 16.5 W, 3.3 ADC. Table general condition Ta = 0 … +45 °C unless otherwise noted.', 5),
        fact('power_description', 'Figure 20 for PFE1100-12-054NA shows nominal main-output current derating dependent on AC input voltage and ambient temperature. Prose says current limitation decreases above 45°C inlet ambient or when AC input is too low, and actual V1 current limitation begins approximately 4 A above Figure 20. No graph points are digitized or transferred from the separate RA Figure 21.', 9),
        fact('cooling_description', 'Series overview states fan cooling; ordering table defines N as Normal and R as Reverse airflow. For the exact NA identity, normal is a printed option label; connector-to-connector direction is not stated on these pages.', '2, 9'),
        fact('physical_description', 'Printed small form factor: 321.5 × 54.5 × 40 mm (12.66 × 2.14 × 1.57 in); dimension order preserved without assigning unprinted axes.', 1),
        fact('management_interface_as_printed', 'I2C communication interface for control/programming/monitoring with PSMI and Power Management Bus protocol; page 2 describes monitoring and fan-speed override via I2C. This is interface documentation, not software installation or entitlement.', '1, 2'),
        fact('document_revision_as_printed', 'BCD.00012_AP; © 2023 Bel Fuse Inc.', 5),
    ], [1, 2, 5, 9], limits=BEL_NAMESPACE),
    row('bel-pfe1100-series.html', 'PFE1100-12-054NA', 'Bel Fuse Inc.', [
        ['model_evidence', 'Saved HTML JSON-LD ProductGroup.hasVariant: name, sku and mpn all exactly PFE1100-12-054NA; isVariantOf.name is PFE1100-12-054xA Series. [HTML application/ld+json ProductGroup.hasVariant exact SKU object]'],
        ['brand_as_printed', 'Bel Fuse. [HTML application/ld+json exact PFE1100-12-054NA hasVariant.brand.name]'],
    ], [], affiliations=[['Bel Fuse', 'documentation_vendor']], limits='The HTML contributes exact SKU/series and printed-brand literals only. Bel Fuse Inc. namespace is retained from the PDF publisher; this HTML does not independently establish that legal name or a manufacturer role. Its documentation-vendor affiliation retains the HTML\'s printed Bel Fuse spelling.'),
    row('bel-pfe1500-series.pdf', 'PFE1500-12-054NAC', 'Bel Fuse Inc.', [
        fact('model_evidence', 'Exact ordering PN PFE1500-12-054NAC is in the upper standard-standby ordering group, separate from the lower S412 group.', 2),
        fact('power_description', 'Exact PFE1500-12-054NAC ordering row: C16 AC input connector; input range 180 VDC ~ 350 VDC and 90 VAC ~ 264 VAC. These operating ranges are distinct from certificate rated-input ranges.', 2),
        fact('power_description', 'Upper standard ordering group has programmable 3.3/5 V standby; lower S412 12 VSB models are separate and excluded from this NAC statement.', 2),
        fact('cooling_description', 'N Normal Airflow from Output connector to Input AC socket; the exact PFE1500-12-054NAC ordering row is under that normal-airflow footnote.', 2),
    ], [2], limits=BEL_NAMESPACE),
    row('bel-pfe1500-nemko.pdf', 'PFE1500-12-054NAC', 'Bel Fuse Inc.', [
        fact('model_evidence', 'PFE1500-12-054NAC is explicitly included in Model / Type Ref. of Nemko certificate No. P22225905, Order No. 456320.', 1),
        fact('certificate_issue_date_as_printed', 'Date of issue 07-01-2022; current validity is not established.', 1),
        fact('manufacturer_as_printed', 'Manufacturer: Bel Fuse Inc.; certificate-scoped legal-manufacturer claim, not physical manufacture of a particular auction unit.', 1),
        fact('applicant_as_printed', 'Applicant: Bel Fuse Inc.', 1),
        fact('factory_as_printed', 'Factory: BPS Asia Pacific Electronics (Shenzhen) Co., Ltd.; separate factory listing, not the manufacturer field or a particular auction unit\'s origin.', 1),
        fact('power_description', 'Certificate rated AC input for listed model group including NAC: 100–240 V ~, 12–7 A, 50/60 Hz.', 1),
        fact('power_description', 'Certificate rated output for listed model group including NAC: V1 12.0 Vdc, 125 A; Vsb alternatives 5 Vdc, 3.3 A / 3.3 Vdc, 5 A.', 1),
        fact('power_description', 'V1 maximum output power is de-rated at different input voltages and operating ambient temperatures; certificate directs the reader to Conditions of Acceptability. It does not establish 125 A at every operating condition.', 1),
        fact('integration_constraint', 'Compliance regarding building-in, electric-shock protection and EMC must be checked when built into a complete product/system; no fitment or system-compliance conclusion follows.', 1),
    ], [1], affiliations=[['Bel Fuse Inc.', 'manufacturer'],
                          ['BPS Asia Pacific Electronics (Shenzhen) Co., Ltd.', 'listed_factory']],
        limits=BEL_NAMESPACE + ' listed_factory is the catalog\'s separate factory-listing role, never a manufacturer alias.'),
    row('bel-pfe1500-csa.pdf', 'PFE1500-12-054NAC', 'Bel Fuse Inc.', [
        fact('model_evidence', 'PFE1500-12-054NAC is explicitly named in separate Canada/United States and outside-Canada/United States model groups; only those NAC groups are transcribed.', 4),
        fact('certificate_reference', 'Certificate 80005631; Project 80111077; Date Issued 2022-01-10.', 4),
        fact('power_description', 'For model groups explicitly including PFE1500-12-054NAC, main output V1 = 12.0 V; output derates linearly as ambient increases 45°C → 60°C and input voltage decreases 200 VAC → 100 VAC. Regional tables remain separate.', 4),
        fact('power_description', 'Canada/United States, NAC-listed model group, 100–200 VAC input: 83.4–125 A at 45°C; 69–104 A at 60°C. These are linear derating ranges, not a constant rating at every input voltage.', 4),
        fact('power_description', 'Canada/United States, NAC-listed model group, 200–240 VAC or 200–350 VDC input: 125 A at 45°C; 104 A at 60°C.', 4),
        fact('power_description', 'Countries other than Canada/United States, NAC-listed model group, 100–200 VAC input: 66.5–125 A at 45°C; 66.5–104 A at 60°C. These are linear derating ranges, not a constant rating at every input voltage.', 4),
        fact('power_description', 'Countries other than Canada/United States, NAC-listed model group, 200–240 VAC or 200–350 VDC input: 125 A at 45°C; 104 A at 60°C.', 4),
    ], [4], limits=BEL_NAMESPACE),
    row('fsp550-29fm.pdf', 'FSP550-29FM', 'FSP', [
        fact('model_evidence', 'Page-1 heading and description both explicitly name FSP550-29FM.', 1),
        fact('power_description', 'Redundant power supply module based on CRPS 2.0; total power capacity up to 550 Watts.', 1),
        fact('power_description', 'Input specifications: 90–264 VAC, 47–63 Hz; input current 115 VAC @ 8 A, 230 VAC @ 4 A. Output rating is separately scoped to 100–240 V input.', 1),
        fact('power_description', 'Output rating at 100–240 V input: +12 V, minimum 1.0 A and maximum 45.8 A; +12 Vsb, minimum 0.0 A and maximum 2.1 A. Rail maxima are separate; no simultaneous summed total is inferred.', 1),
        fact('physical_description', 'Dimension (L × W × H): 185 × 73.5 × 39 mm; 7.28 × 2.89 × 1.53 inch. These are page-1 specifications, not the disputed page-2 drawing.', 1),
        fact('physical_description', 'Working temperature: 0°C to 55°C.', 1),
        fact('management_interface_as_printed', 'Meet PMBus 1.2; description says PMbus features allow communication with motherboard. This is interface documentation, not software installation or entitlement.', 1),
        fact('document_revision_as_printed', '2020.05 spec; © 2019 FSP Group.', 1),
        fact('source_identity_conflict', 'Page 2 visibly names FSP550-20FM; its drawing and airflow are excluded from FSP550-29FM. The -29FM filename does not resolve the conflict.', 2),
    ], [1, 2], limits='FSP is the printed logo/vendor namespace, not a per-model manufacturer certificate. Page-2 -20FM drawing/airflow excluded. No other product-page MTBF statement is imported.'),
]


def main():
    captures = {entry['file']: entry for entry in json.loads((CAPTURES / 'fetch-report.json').read_text())}
    sources, snapshots = [], []
    (HERE / 'snapshots').mkdir(exist_ok=True)
    for device in devices:
        filename = next(name for name in captures if device['source'] == source_id(name))
        entry = captures[filename]
        original = CAPTURES / filename
        expected = entry['sha256']
        assert hashlib.sha256(original.read_bytes()).hexdigest() == expected
        copied = HERE / 'snapshots' / filename
        if copied.exists():
            assert copied.read_bytes() == original.read_bytes(), 'Never replace a changed snapshot'
        else:
            shutil.copy2(original, copied)
        assert hashlib.sha256(copied.read_bytes()).hexdigest() == expected
        certification = 'nemko' in filename or 'csa' in filename
        coverage = device['scope_limitations']
        source = {'id': device['source'], 'url': entry['url'],
                  'tier': 'certification_body' if certification else 'vendor',
                  'license': 'Publicly readable factual document; upstream rights retained; no redistribution permission inferred.',
                  'snapshot': copied.relative_to(HERE).as_posix(), 'coverage': coverage,
                  'original_sha256': expected, 'captured_at': entry['captured_at'],
                  'final_url': entry['final_url']}
        sources.append(source)
        snapshots.append({'source_id': source['id'], 'original_path': str(original.relative_to(HERE.parent)),
                          'snapshot': source['snapshot'], 'sha256': expected, 'bytes': copied.stat().st_size,
                          'url': source['url'], 'reviewed_pdf_pages': device['reviewed_pdf_pages']})
    candidate = {'sources': sources, 'devices': devices,
                 'candidate_status': 'staged; independent fact review and integration checks required',
                 'scope_limitations': LIMITS}
    assert len({s['id'] for s in sources}) == len(sources)
    assert len({s['snapshot'] for s in sources}) == len(sources)
    assert all(sum(d['source'] == s['id'] for d in devices) == 1 for s in sources)
    (HERE / 'candidate.json').write_text(json.dumps(candidate, ensure_ascii=False, indent=2) + '\n')
    (HERE / 'provenance.json').write_text(json.dumps({'snapshots': snapshots}, ensure_ascii=False, indent=2) + '\n')
    print('staged', len(sources), 'sources,', len(devices), 'records,',
          len({(d['brand'], d['model']) for d in devices}), 'identities,',
          sum(len(d['facts']) for d in devices), 'facts; no imports')


if __name__ == '__main__':
    main()
