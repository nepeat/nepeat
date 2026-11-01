"""One-shot reviewed finite release staging; refuses existing release outputs."""
import datetime
import json
from pathlib import Path

from devicehunt.archive import freeze, stable_receipt
from devicehunt.artifacts import Archive
from devicehunt.derivations import Derivations
from devicehunt import lineage

ROOT = Path.cwd()
BATCH = ROOT / 'research/batches/archive-implementation-2026-10-04'
archive = Archive(ROOT / 'data/archive', workspace=ROOT)
derivations = Derivations(archive)
documents = archive.receipts('reference-document')
golduck = next(r for r in documents if r['payload']['entry']['id'] == 'inventec-golduck-product-brief-v3-2020')
text_sha = golduck['payload']['text_sha256']
inputs = [{'role': 'page-text', 'sha256': text_sha}]
recipe = derivations.recipe('page-text-flatten', '1')
first = derivations.run(recipe, inputs, consumer='offline-reading')
second = derivations.run(recipe, inputs, consumer='later-question-extraction')
assert first['id'] == second['id']
changed = derivations.run(derivations.recipe('page-text-flatten', '1', {'separator': '\n\n'}),
                          inputs, consumer='paragraph-reading')
assert changed['id'] != first['id']
xml_path = ROOT / 'data/raw/fcc-sitemaps/fccid-co-sitemap.xml'
xml_sha = archive.register_file(xml_path)
xml_inputs = [{'role': 'sitemap-index', 'sha256': xml_sha}]
xml_recipe = derivations.recipe('xml-locations', '1')
xml1 = derivations.run(xml_recipe, xml_inputs, consumer='collection-inventory')
xml2 = derivations.run(xml_recipe, xml_inputs, consumer='coverage-review')
assert xml1['id'] == xml2['id']

curated_path = ROOT / 'data/inventec-practical-facts-2026-10-04.json'
curated_sha = archive.register_file(curated_path)
proof_path = ROOT / 'research/batches/inventec-practical-2026-10-04/golduck/fact-provenance.json'
proof_sha = archive.register_file(proof_path)
review_path = ROOT / 'research/batches/inventec-practical-2026-10-04/source-review.md'
review_sha = archive.register_file(review_path)
device = next(d for d in json.loads(curated_path.read_bytes())['devices'] if d['key'] == 'Golduck')
proofs = {p['fact_name']: p for p in json.loads(proof_path.read_bytes())}
selected = ('form_factor', 'power_supply', 'cooling_description', 'pcie_lanes_as_printed')
statements, reviews, links = [], [], []
review_time = datetime.datetime.now(datetime.timezone.utc).isoformat()
for name, value in device['facts']:
    if name not in selected:
        continue
    proof = proofs[name]
    assert value == proof['literal_quote']
    statement = dict(claim_id=lineage.identity(['devicehunt-reviewed-v1', device['source'], device['key'], name]),
                     source_id=device['source'], source_key=device['key'], predicate=name,
                     raw_value=value, normalized_value=None, role='vendor_specification',
                     projection_kind='fact', projection={'name': name, 'value': value})
    statement['statement_id'] = lineage.identity(statement)
    review = dict(statement_id=statement['statement_id'], decision='accepted',
                  reviewer='archive-coordinator', reviewed_at=review_time,
                  reason='Existing reviewed source fact and exact PDF/page-text association preserved. '
                         'See pinned prior source-review and fact-provenance artifacts. '
                         'Printed width-unit conflict and 162-lane wording remain literal; no new interpretation.')
    review['review_id'] = lineage.identity(review)
    statement['accepted_review_id'] = review['review_id']
    link = dict(statement_id=statement['statement_id'],
                artifact_sha256=golduck['payload']['artifact_sha256'], receipt_id=golduck['id'],
                locator={'kind': 'pdf_page', 'page': proof['pdf_page'], 'text_sha256': text_sha,
                         'table_row': proof['location'], 'page_convention': 'one-based PDF index'},
                excerpt=proof['literal_quote'])
    link['link_id'] = lineage.identity(link)
    statements.append(statement)
    reviews.append(review)
    links.append(link)
stable_receipt(archive, 'review-basis', {'artifacts': [{'sha256': s} for s in
               (curated_sha, proof_sha, review_sha)], 'selected_claims': [s['claim_id'] for s in statements]})
release_path = ROOT / 'data/archive-release-2026-10-04.json'
release = freeze(archive, release_path, statements, reviews, links)
summary = {'release_id': release['release_id'], 'path': str(release_path.relative_to(ROOT)),
           'sha256': lineage.checksum(release_path), 'artifacts': len(release['artifacts']),
           'receipts': len(release['receipts']), 'statements': len(statements),
           'reuse': {'pdf_consumers': 2, 'shared_pdf_run': first['id'],
                     'changed_recipe_run': changed['id'], 'xml_consumers': 2,
                     'shared_xml_run': xml1['id'], 'network_requests': 0},
           'audit': archive.audit()}
with (BATCH / 'release-proof.json').open('x') as stream:
    json.dump(summary, stream, indent=2)
print(json.dumps(summary, indent=2))
archive.close()
