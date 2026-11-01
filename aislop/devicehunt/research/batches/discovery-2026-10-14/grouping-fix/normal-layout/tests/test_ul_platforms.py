import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import ul_platforms as ul
from devicehunt.catalog import Catalog


class UlPlatformTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def save(self, text, alter=None):
        data = dict(schema_version=1, capture_format='web_tool_models_section',
                    source_url='https://productiq.ulprospector.com/en/profile/2858881/azot7.e141136?term=e141136',
                    document=ul.DOCUMENT, listing_holder=ul.HOLDER, section='Models',
                    extraction_scope=ul.SCOPE, retrieved_at='2026-09-26 22:57:28 UTC',
                    model_section_text=text, models=sorted(ul.extract(text)))
        if alter:
            alter(data)
        p = self.root / 'capture.json'; p.write_text(json.dumps(data)); return p

    def test_full_qualifiers_are_not_reduced_to_base_models(self):
        text = 'AS9516-32D (16-Core) DMC-12-48R-AS0 48V ECS4100-12T v2 ECS4100-28TC-F US2 AS5915-16X AC AZ4444-52X-DC-42'
        expected = ['AS9516-32D (16-Core)', 'DMC-12-48R-AS0 48V', 'ECS4100-12T v2',
                    'ECS4100-28TC-F US2', 'AS5915-16X AC', 'AZ4444-52X-DC-42']
        self.assertEqual(list(ul.extract(text)), expected)
        for model, spans in ul.extract(text).items():
            for start, end in spans:
                self.assertEqual(text[start:end], model)
        self.assertNotIn('AZ4444-52X', ul.extract(text))

    def test_named_platforms_duplicate_mentions_and_wildcards(self):
        models = ul.extract('Minipack-AS8000 Minipack Barefoot DNA X1032C ORV3 HSM Wedge100BF-65X ASXvOLT16 ASGvOLT64 ASA926-18XKE ASXvOLT16 FS-648Fxxxxxxxxxx AS9999-xx')
        self.assertEqual(len(models), 8)
        self.assertEqual(len(models['ASXvOLT16']), 2)
        self.assertNotIn('AS8000', models)
        self.assertNotIn('AS9999-xx', models)

    def test_unreviewed_spaced_qualifier_fails(self):
        for text in ['AS9516-32D (32-Core)', 'DMC-12-48R-AS0 24V', 'ECS4100-12T v3']:
            with self.assertRaisesRegex(ValueError, 'qualifier'):
                ul.extract(text)

    def test_commercial_codes_preserve_variants_and_do_not_match_token_fragments(self):
        text = ('EX4100-48T EX4100-F-48T ACX7024 ACX7024X QFX5241-32OD '
                'JNP7100-32C ACX7100-32C SRX4120 JCP-JNET2-ID EX4100-48T '
                'other-EX4100-48T EX4100-48T_extra QFX5241-32OD.rev '
                'SRX9999-xxxx FORTISWITCH-648Fxxxx')
        models = ul.extract_commercial(text)
        self.assertEqual(set(models), {'EX4100-48T', 'EX4100-F-48T', 'ACX7024',
                         'ACX7024X', 'QFX5241-32OD', 'JNP7100-32C', 'ACX7100-32C',
                         'SRX4120', 'JCP-JNET2-ID'})
        self.assertEqual(len(models['EX4100-48T']), 2)
        for model, spans in models.items():
            for start, end in spans:
                self.assertEqual(text[start:end], model)

    def test_commercial_import_keeps_holder_role_and_distinct_identities(self):
        from devicehunt.listings import Matcher
        text = 'AS7760-32X EX4100-48T EX4100-F-48T ACX7100-32C JNP7100-32C'
        p = self.save(text)
        for _ in range(2):
            self.assertEqual(ul.import_capture(self.c, p), {'model_assertions': 5})
        self.assertEqual(len(self.c.search(target='Accton', role='listing_holder')), 5)
        for role in ['manufacturer', 'brand', 'listed_factory']:
            self.assertEqual(self.c.search(role=role), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        raw = self.c.db.execute("SELECT raw_json FROM records WHERE source_key='EX4100-48T'").fetchone()
        self.assertEqual(json.loads(raw[0])['extraction_scope'], ul.COMMERCIAL_SCOPE)
        result = Matcher(self.c.db, 'Accton').match('Juniper EX4100-48T switch', 20)
        self.assertEqual(result['status'], 'mentions_found')

    def test_commercial_qualifier_rejection_preserves_existing_source(self):
        ul.import_capture(self.c, self.save('AS7760-32X'))
        for text in ['AS7760-32X EX4100-48T (24V)', 'AS7760-32X QFX5241-32OD DC']:
            with self.assertRaisesRegex(ValueError, 'qualifier'):
                ul.import_capture(self.c, self.save(text))
            self.assertEqual(len(self.c.search('AS7760-32X')), 1)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 1)

    def test_listing_holder_does_not_imply_manufacture_or_deployment(self):
        p = self.save('ORV3 HSM AS7760-32X AZ4444-52X-DC-42')
        for _ in range(2):
            self.assertEqual(ul.import_capture(self.c, p), {'model_assertions': 3})
        self.assertEqual(len(self.c.search('ORV3 HSM', target='Accton')), 1)
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 3)

    def test_extended_models_keep_names_and_adjacent_annotations_separate(self):
        text = ('JL675A, R8N85A (RMN ID:RSVLC-1810) S1A527AB '
                'Vision E100 DC Vision E100 AC Nile NSW200 GigaVUE-TA25E '
                'JL675A S0M83A (RMN ID: RSVLC- 2205) R8R46A (RMNRSVLC-2111)')
        models, notes = ul.extract_extended(text)
        self.assertEqual(set(models), {'JL675A', 'R8N85A', 'S1A527AB', 'Vision E100 DC',
                         'Vision E100 AC', 'Nile NSW200', 'GigaVUE-TA25E', 'S0M83A', 'R8R46A'})
        self.assertEqual(len(models['JL675A']), 2)
        self.assertNotIn('JL675A', notes)
        self.assertEqual(notes['R8N85A'], {'(RMN ID:RSVLC-1810)'})
        self.assertEqual(notes['S0M83A'], {'(RMN ID: RSVLC- 2205)'})
        for model, spans in models.items():
            for start, end in spans:
                self.assertEqual(text[start:end], model)

    def test_extended_scope_avoids_templates_fragments_and_existing_scopes(self):
        text = ('ECS4100-12T v2 AS7772-32X ACX7024 FS-AX648Fxxxxxxxxxx '
                'FortiSwitch-AX648Fxxxxxxxxxx ISV8xxx-xxx K60DNxy 7850 '
                '1FINITYTM T600 BDT6-T600 JL680A_extra JL680A.rev other-JL680A '
                'FS-AX2340S-48P4X FortiSwitch-AX2340S-48P4X AX2340S-48P4X')
        models, _ = ul.extract_extended(text)
        self.assertEqual(set(models), {'FS-AX2340S-48P4X', 'FortiSwitch-AX2340S-48P4X', 'AX2340S-48P4X'})

    def test_extended_import_annotations_are_searchable_but_not_identity_aliases(self):
        from devicehunt import validation
        path = self.save('AS7772-32X JL675A, R8N85A (RMN ID:RSVLC-1810) GigaVUE-TA25E')
        for _ in range(2):
            self.assertEqual(ul.import_capture(self.c, path), {'model_assertions': 4})
        hit = self.c.search('RSVLC-1810')[0]
        self.assertEqual(hit['model'], 'R8N85A')
        detail = self.c.detail(hit['id'])
        self.assertEqual({a['role'] for a in detail['affiliations']}, {'listing_holder'})
        self.assertEqual({i['kind'] for i in detail['identifiers']}, {'ul_document'})
        self.assertEqual(self.c.search(hyperscaler=True), [])
        inventory = self.root / 'inventory.json'
        inventory.write_text(json.dumps({'schema_version': 1, 'devices': [
            {'identifier': 'RSVLC-1810', 'vendor': 'Accton'}]}))
        result = validation.run(self.c.db, inventory)
        self.assertEqual(result['summary']['hits'], 0)
        ul.import_capture(self.c, self.save('AS7772-32X'))
        self.assertEqual(self.c.search('RSVLC-1810'), [])

    def test_unknown_extended_qualifiers_leave_import_unchanged(self):
        ul.import_capture(self.c, self.save('AS7772-32X'))
        for model in ['JL680A (RMN ID: UNKNOWN)', 'JL680A(new)', 'GigaVUE-TA25E DC',
                      'Vision E50 24V', 'R8R46A (RMNRSVLC-2111) (new)']:
            with self.assertRaisesRegex(ValueError, 'qualifier'):
                ul.import_capture(self.c, self.save('AS7772-32X ' + model))
            self.assertEqual(len(self.c.search()), 1)

    def test_wrong_holder_missing_models_and_modified_extraction_preserve_existing_evidence(self):
        ul.import_capture(self.c, self.save('AS7760-32X'))
        for alteration in [lambda d: d.update(listing_holder='UNRELATED COMPANY'),
                           lambda d: d.update(models=['AS7760']),
                           lambda d: d.update(model_section_text='AS7772-32X'),
                           lambda d: d.update(extraction_scope='something else')]:
            with self.assertRaises(ValueError):
                ul.import_capture(self.c, self.save('AS7760-32X', alteration))
            self.assertEqual(len(self.c.search('AS7760-32X')), 1)
