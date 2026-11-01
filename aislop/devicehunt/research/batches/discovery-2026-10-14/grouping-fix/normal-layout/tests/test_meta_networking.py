import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import meta_networking as meta
from devicehunt import hyperscaler_inventory
from devicehunt.catalog import Catalog, digest


CONTENT = {
    'meta-minipack-2019': '''<p>We worked with Edgecore to build Minipack based on our designs. 128x100GE.</p>
      <p>eight port interface module: PIM-16Q, 8x200G QSFP56, remaining eight ports inactive; PIM-4DD, 4x400G QSFP-DD.</p>
      <p>we use FBOSS on Minipack; external Cumulus Networks and SAI/SONIC ports.</p>''',
    'meta-ocp-2021': '''<p>Both the Wedge400 and 400C are manufactured by Celestica; already been deployed in our data centers. 12.8 Tbps; field-replaceable CPU.</p>
      <p>Wedge 400 utilizes Broadcom Tomahawk 3; 400C uses Cisco Silicon One.</p>''',
    'meta-ocp-2024': '''<figure><figcaption>Minipack3 (Broadcom Tomahawk5 based, designed by Meta and manufactured by Celestica) 51.2T.</figcaption></figure>
      <p>Meta will deploy Minipack3.</p><p>Minipack3 utilizes Tomahawk5: 51.2 Tbps, 64x OSFP.</p>
      <p>Meta will run Minipack3 on FBOSS.</p>''',
    'meta-ocp-2025': '''<figure><figcaption>Minipack3N, designed by Meta and manufactured by Accton. 51.2 Tbps, NVIDIA Spectrum-4.</figcaption></figure>
      <p>introducing Minipack3N with NVIDIA Spectrum-4, same system design as Minipack3.</p>
      <p>Minipack3 (based on Broadcom Tomahawk5) and Cisco 8501.
      These platforms have served as the foundation for building our next-generation frontend and backend data center fabrics.</p>''',
}


def html(sid, content):
    timestamp = '2025-10-14T00:00:20+00:00' if sid == 'meta-ocp-2025' else meta.DATES[sid] + 'T12:00:00+00:00'
    return ('<html><meta property="og:url" content="' + meta.URLS[sid] + '">'
            '<meta property="article:published_time" content="' + timestamp + '">'
            '<time class="published updated" datetime="' + meta.DATES[sid] + '"></time>'
            '<div class="entry-content">' + content + '</div></html>')


class MetaNetworkingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def bundle(self, alter=None):
        bodies = {sid: html(sid, content) for sid, content in CONTENT.items()}
        if alter:
            alter(bodies)
        data = {'schema_version': 1, 'articles': {}}
        for sid, body in bodies.items():
            p = self.root / (sid + '.html'); p.write_text(body)
            data['articles'][sid] = dict(path=p.name, url=meta.URLS[sid], sha256=digest(p.read_bytes()))
        p = self.root / 'bundle.json'; p.write_text(json.dumps(data)); return p

    def test_manufacturers_chips_and_deployment_scopes_remain_model_specific(self):
        p = self.bundle()
        for _ in range(2):
            self.assertEqual(meta.import_bundle(self.c, p), {'sources': 4, 'model_assertions': 6})
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 5)
        accton = self.c.search(target='Accton', role='manufacturer')
        self.assertEqual([d['model'] for d in accton], ['Minipack3N'])
        self.assertEqual({d['model'] for d in self.c.search(target='Celestica', role='manufacturer')},
                         {'Wedge 400', 'Wedge 400C', 'Minipack3'})
        self.assertEqual(self.c.search(target='Edge-Core', role='manufacturer'), [])
        rows = self.c.db.execute('SELECT d.model,h.relationship FROM hyperscaler_evidence h JOIN records r ON r.id=h.record_id JOIN devices d ON d.id=r.device_id')
        self.assertEqual({r[1] for r in rows if r[0] == 'Minipack3'},
                         {'announced_platform_deployment', 'documented_fabric_role'})
        self.assertEqual(len(self.c.search(hyperscaler=True)), 5)
        chip = self.c.db.execute("SELECT f.value FROM facts f JOIN records r ON r.id=f.record_id JOIN devices d ON d.id=r.device_id WHERE d.model='Wedge 400C' AND f.name='asic'").fetchone()[0]
        self.assertEqual(chip, 'Cisco Silicon One')

    def test_qualifications_and_both_publication_dates_are_retained(self):
        meta.import_bundle(self.c, self.bundle())
        facts = list(self.c.db.execute('SELECT name,value FROM facts'))
        self.assertIn(('published_date', '2025-10-13'), [tuple(r) for r in facts])
        self.assertIn(('published_timestamp', '2025-10-14T00:00:20+00:00'), [tuple(r) for r in facts])
        self.assertTrue(any('eight ports are inactive' in row[1] for row in facts))
        self.assertFalse(self.c.search('Minipack2'))

    def test_wrong_manufacturer_date_or_redirect_rejected_before_replacement(self):
        meta.import_bundle(self.c, self.bundle())
        for old, new in [('manufactured by Accton', 'manufactured by Another Vendor'),
                         ('datetime="2025-10-13"', 'datetime="2026-10-13"'),
                         (meta.URLS['meta-ocp-2025'], 'https://engineering.fb.com/')]:
            def alter(bodies):
                bodies['meta-ocp-2025'] = bodies['meta-ocp-2025'].replace(old, new)
            with self.assertRaises(ValueError): meta.import_bundle(self.c, self.bundle(alter))
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 6)

    def test_fabric_role_does_not_borrow_manufacturer_or_upgrade_announced_deployment(self):
        meta.import_bundle(self.c, self.bundle())
        result = hyperscaler_inventory.run(self.c.db, relationship='documented_fabric_role')
        self.assertEqual(result['assertions'], 1)
        row = result['rows'][0]
        self.assertEqual(row['model'], 'Minipack3')
        self.assertEqual(row['affiliations'], [dict(company='Meta', target=None, role='documentation_vendor')])
        for vendor in ('Celestica', 'Accton'):
            self.assertEqual(hyperscaler_inventory.run(self.c.db, vendor=vendor,
                             relationship='documented_fabric_role')['assertions'], 0)
        self.assertEqual(hyperscaler_inventory.run(self.c.db,
                         relationship='documented_platform_deployment')['assertions'], 2)
        self.assertEqual(hyperscaler_inventory.run(self.c.db,
                         relationship='announced_platform_deployment')['assertions'], 1)

    def test_fabric_statement_and_identity_basis_must_both_validate_before_refresh(self):
        meta.import_bundle(self.c, self.bundle())
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM records')]
        for sid, old, new in [
                ('meta-ocp-2025', 'Minipack3 (based on', 'Minipack3N (based on'),
                ('meta-ocp-2025', 'have served', 'will serve'),
                ('meta-ocp-2025', 'These platforms have served', 'These platforms have not served; These platforms have served'),
                ('meta-ocp-2024', 'manufactured by Celestica', 'manufactured by Another Vendor')]:
            def alter(bodies): bodies[sid] = bodies[sid].replace(old, new)
            with self.assertRaises(ValueError): meta.import_bundle(self.c, self.bundle(alter))
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')], before)

    def test_footer_script_and_image_alt_text_cannot_supply_required_caption(self):
        sid = 'meta-ocp-2025'; body = html(sid, CONTENT[sid])
        start = body.index('<figure>'); end = body.index('</figure>') + len('</figure>')
        removed, caption = body[:start] + body[end:], body[start:end]
        for modified in [removed.replace('</html>', caption + '</html>'),
                         removed.replace('</div>', '<script>' + caption + '</script></div>'),
                         removed.replace('</div>', '<img alt="' + caption.replace('"', '') + '"></div>')]:
            with self.assertRaises(ValueError): meta.reviewed_records(modified, sid)

    def test_dependency_checksum_and_negated_claim_fail(self):
        p = self.bundle(); (self.root / 'meta-ocp-2025.html').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'checksum'): meta.import_bundle(self.c, p)
        def negate(bodies):
            bodies['meta-ocp-2021'] = bodies['meta-ocp-2021'].replace('already been deployed', 'not been deployed; already been deployed')
        with self.assertRaisesRegex(ValueError, 'Negated'): meta.import_bundle(self.c, self.bundle(negate))
