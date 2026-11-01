import json
import unittest

from devicehunt import lookup
from tests import test_lookup


class LookupBatchTests(unittest.TestCase):
    setUp = test_lookup.LookupTests.setUp
    tearDown = test_lookup.LookupTests.tearDown

    def test_batch_equals_single_queries_with_literal_matching_limits_and_raw_data(self):
        requests = [(q, limit) for q in [' Shared-Model ', 'STRASSE_100%',
                    'Different\nManufacturer', '100% load', 'LISTING-REF',
                    "' OR 1=1 --", 'absent'] for limit in [0, 1, 20, -1]]
        self.assertEqual(lookup.run_many(self.c.db, requests),
                         [lookup.run(self.c.db, *request) for request in requests])
        self.assertEqual(len(lookup.run_many(self.c.db, [('Shared-Model', 20)])[0]['datasets']),
                         1 + len(lookup.REGISTRIES))

    def test_batch_preserves_missing_tables_zero_sources_and_stale_fts_membership(self):
        db = self.c.db
        did = db.execute('SELECT id FROM devices').fetchone()[0]
        for key in [did, did, 'missing-device']:
            db.execute('INSERT INTO search_index(device_id,text) VALUES (?,?)', (key, 'extra'))
        db.execute('DROP TABLE fcc_grantees')
        self.c.source('rra-official-query-zero', 'zero', 'regulatory', 'fixture', 'verified zero')
        requests = [('extra', -1), ('Shared-Model', 1), ('missing', 0)]
        results = lookup.run_many(db, requests)
        self.assertEqual(results, [lookup.run(db, *request) for request in requests])
        self.assertEqual(results[0]['datasets'][0]['matches'], 1)
        sections = {s['dataset']: s for s in results[0]['datasets']}
        self.assertEqual(sections['rra']['state'], 'loaded')
        self.assertIn('No table', sections['fcc_grantees']['scope'])

    def test_new_call_reads_changes_and_preserves_caller_transaction_on_success_and_error(self):
        db = self.c.db
        initial = lookup.run_many(db, [('Changed', -1)])
        self.assertEqual(initial[0]['datasets'][0]['matches'], 0)
        db.execute("UPDATE devices SET title='Changed'")
        updated = lookup.run_many(db, [('Changed', -1)])
        self.assertEqual(updated[0]['datasets'][0]['matches'], 1)
        self.assertTrue(db.in_transaction)
        db.execute("UPDATE equipment_register SET cells_json='invalid json'")
        with self.assertRaises(Exception):
            lookup.run_many(db, [('Changed', -1)])
        self.assertTrue(db.in_transaction)
        self.assertEqual(db.execute('SELECT title FROM devices').fetchone()[0], 'Changed')
        db.rollback()

    def test_validation_precedes_all_queries_and_owned_snapshot_closes(self):
        db = self.c.db
        db.commit()
        statements = []
        db.set_trace_callback(statements.append)
        for request in [(' ', 20), ('ok', True), ('ok', -2)]:
            with self.assertRaises(ValueError):
                lookup.run_many(db, [('valid', 20), request])
        self.assertEqual(statements, [])
        self.assertEqual(lookup.run_many(db, []), [])
        lookup.run_many(db, [('Shared-Model', 20)])
        self.assertFalse(db.in_transaction)
        db.set_trace_callback(None)

    def test_registry_order_ties_duplicates_null_cells_and_raw_dates(self):
        db = self.c.db
        db.execute("INSERT INTO equipment_register VALUES (?,?,?,?,?,?,?,?,?,?)",
                   ['icasa-2025', 42, 8, None, None, None, None, 'review_required',
                    json.dumps(['Shared-Model', None, '']), 'second'])
        self.c.source('second-anatel', 'second', 'regulatory', 'fixture', 'tie')
        row = list(db.execute('SELECT * FROM anatel_equipment WHERE row_number=1').fetchone())
        row[0] = 'second-anatel'
        db.execute('INSERT INTO anatel_equipment VALUES (' + ','.join('?' for _ in row) + ')', row)
        requests = [('Shared-Model', limit) for limit in [0, 1, 20, -1]]
        self.assertEqual(lookup.run_many(db, requests), [lookup.run(db, *r) for r in requests])
