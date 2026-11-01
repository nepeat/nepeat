"""Rebuild a catalog from a reviewed local snapshot manifest without network I/O."""
import hashlib
import json
import os
import re
import tempfile
from pathlib import Path

from .catalog import Catalog
from .pipeline import SOURCE_NAMES, import_source
from .review_queue import gate_inputs, gate_projections


def file_sha256(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def local_path(root, name):
    if not isinstance(name, str) or not name or Path(name).is_absolute():
        raise ValueError('Snapshot path must be relative: ' + str(name))
    path = (root / name).resolve()
    if root not in path.parents:
        raise ValueError('Snapshot path escapes manifest directory: ' + name)
    return path


def read_manifest(path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    if (not isinstance(data, dict) or data.get('schema_version') != 1
            or not isinstance(data.get('imports'), list) or not data['imports']
            or not isinstance(data.get('files'), dict) or not data['files']):
        raise ValueError('Rebuild manifest requires schema_version 1, imports and files')
    root = path.parent
    for name, sha in data['files'].items():
        if not isinstance(sha, str) or not re.fullmatch(r'[0-9a-f]{64}', sha):
            raise ValueError('Invalid snapshot hash: ' + name)
        if file_sha256(local_path(root, name)) != sha:
            raise ValueError('Snapshot checksum mismatch: ' + name)
    seen = set()
    for job in data['imports']:
        if not isinstance(job, dict) or not isinstance(job.get('source'), str) or not isinstance(job.get('path'), str):
            raise ValueError('Import jobs require source and path strings')
        if job['source'] not in SOURCE_NAMES or job['path'] not in data['files']:
            raise ValueError('Unknown importer or unhashed input: ' + str(job))
        key = (job['source'], job['path'])
        if key in seen:
            raise ValueError('Duplicate import: ' + str(key))
        seen.add(key)
    if 'campaign_gate' in data:
        # Reviewed campaign units: inputs must equal the frozen release before any import.
        gate_inputs(root, data)
    expected = data.get('expected')
    if not isinstance(expected, dict) or not isinstance(expected.get('sources'), dict):
        raise ValueError('Manifest requires expected source counts')
    expected.setdefault('grantees', 0)
    expected.setdefault('equipment_register', 0)
    expected.setdefault('bahrain_equipment', 0)
    expected.setdefault('anatel_equipment', 0)
    expected.setdefault('ntra_equipment', 0)
    expected.setdefault('tec_equipment', 0)
    expected.setdefault('ieee_assignments', 0)
    expected.setdefault('pci_entries', 0)
    expected.setdefault('ncc_equipment', 0)
    expected.setdefault('bsmi_certificates', 0)
    expected.setdefault('bsmi_product_certificates', 0)
    expected.setdefault('bsmi_voluntary_certificates', 0)
    expected.setdefault('bsmi_declarations', 0)
    expected.setdefault('rra_equipment', 0)
    expected.setdefault('energystar_servers', 0)
    expected.setdefault('energystar_server_archives', 0)
    expected.setdefault('energystar_datacenter', 0)
    expected.setdefault('ised_equipment', 0)
    expected.setdefault('nigeria_equipment', 0)
    expected.setdefault('ghana_equipment', 0)
    expected.setdefault('public_equipment_rows', 0)
    expected.setdefault('japan_equipment', 0)
    expected.setdefault('inspur_support_entries', 0)
    if type(expected['inspur_support_entries']) is not int or expected['inspur_support_entries'] < 0:
        raise ValueError('Invalid expected Inspur support count')
    if type(expected['japan_equipment']) is not int or expected['japan_equipment'] < 0:
        raise ValueError('Invalid expected Japanese register count')
    if type(expected['public_equipment_rows']) is not int or expected['public_equipment_rows'] < 0:
        raise ValueError('Invalid expected public register count')
    expected.setdefault('fcc_id_index', 0)
    expected.setdefault('fcc_company_rows', 0)
    for key in ('fcc_sitemap_pages', 'fcc_sitemap_urls'):
        expected.setdefault(key, 0)
        if type(expected[key]) is not int or expected[key] < 0:
            raise ValueError('Invalid expected sitemap count')
    if type(expected['fcc_company_rows']) is not int or expected['fcc_company_rows'] < 0:
        raise ValueError('Invalid expected FCC company row count')
    expected.setdefault('documents', 0)
    expected.setdefault('document_pages', 0)
    for key in ('fcc_detail_pages', 'fcc_detail_links', 'fcc_text_excerpts'):
        expected.setdefault(key, 0)
    from .lineage import TABLES
    for table in TABLES:
        expected.setdefault(table, 0)
        if type(expected[table]) is not int or expected[table] < 0:
            raise ValueError('Invalid expected lineage count: ' + table)
    for key in ('documents', 'document_pages', 'fcc_detail_pages', 'fcc_detail_links', 'fcc_text_excerpts', 'tec_equipment', 'pci_entries', 'ncc_equipment', 'bsmi_certificates', 'bsmi_product_certificates', 'bsmi_voluntary_certificates', 'bsmi_declarations', 'rra_equipment', 'energystar_servers', 'energystar_server_archives', 'energystar_datacenter', 'ised_equipment'):
        if type(expected[key]) is not int or expected[key] < 0:
            raise ValueError('Invalid expected document count')
    for count in [expected.get('devices'), expected.get('records'), expected['grantees'], expected['equipment_register'], expected['bahrain_equipment'], expected['anatel_equipment'], expected['ntra_equipment'], expected['ieee_assignments'], expected['fcc_id_index'], *expected['sources'].values()]:
        if type(count) is not int or count < 0:
            raise ValueError('Invalid expected count')
    return root, data


def rebuild(manifest, destination, progress=None):
    destination = Path(destination).resolve()
    if destination.exists():
        raise ValueError('Rebuild destination already exists; choose a new database path')
    root, data = read_manifest(manifest)
    release = gate_inputs(root, data) if 'campaign_gate' in data else None
    destination.parent.mkdir(parents=True, exist_ok=True)
    # Imports commit individually, so build in a disposable staging directory.
    # Publication is exclusive: another writer's destination is never replaced.
    with tempfile.TemporaryDirectory(prefix='.devicehunt-build-', dir=destination.parent) as work:
        staging = Path(work) / 'catalog.sqlite'
        c = Catalog(staging)
        try:
            for job in data['imports']:
                result = import_source(c, job['source'], local_path(root, job['path']))
                if progress:
                    progress({'source': job['source'], 'result': result})
            if release is not None:
                gate_projections(c.db, release)
            actual = {
                'devices': c.db.execute('SELECT count(*) FROM devices').fetchone()[0],
                'records': c.db.execute('SELECT count(*) FROM records').fetchone()[0],
                'sources': dict(c.db.execute('SELECT id,records_seen FROM sources')),
                'grantees': c.db.execute('SELECT count(*) FROM fcc_grantees').fetchone()[0],
                'equipment_register': c.db.execute('SELECT count(*) FROM equipment_register').fetchone()[0],
                'bahrain_equipment': c.db.execute('SELECT count(*) FROM bahrain_equipment').fetchone()[0],
                'anatel_equipment': c.db.execute('SELECT count(*) FROM anatel_equipment').fetchone()[0],
                'ntra_equipment': c.db.execute('SELECT count(*) FROM ntra_equipment').fetchone()[0],
                'tec_equipment': c.db.execute('SELECT count(*) FROM tec_equipment').fetchone()[0],
                'ieee_assignments': c.db.execute('SELECT count(*) FROM ieee_assignments').fetchone()[0],
                'pci_entries': c.db.execute('SELECT count(*) FROM pci_entries').fetchone()[0],
                'bsmi_certificates': c.db.execute('SELECT count(*) FROM bsmi_certificates').fetchone()[0],
                'bsmi_product_certificates': c.db.execute('SELECT count(*) FROM bsmi_product_certificates').fetchone()[0],
                'bsmi_voluntary_certificates': c.db.execute('SELECT count(*) FROM bsmi_voluntary_certificates').fetchone()[0],
                'bsmi_declarations': c.db.execute('SELECT count(*) FROM bsmi_declarations').fetchone()[0],
                'rra_equipment': c.db.execute('SELECT count(*) FROM rra_equipment').fetchone()[0],
                'energystar_servers': c.db.execute('SELECT count(*) FROM energystar_servers').fetchone()[0],
                'energystar_server_archives': c.db.execute('SELECT count(*) FROM energystar_server_archives').fetchone()[0],
                'energystar_datacenter': c.db.execute('SELECT count(*) FROM energystar_datacenter').fetchone()[0],
                'ised_equipment': c.db.execute('SELECT count(*) FROM ised_equipment').fetchone()[0],
                'nigeria_equipment': c.db.execute('SELECT count(*) FROM nigeria_equipment').fetchone()[0],
                'ghana_equipment': c.db.execute('SELECT count(*) FROM ghana_equipment').fetchone()[0],
                'public_equipment_rows': c.db.execute('SELECT count(*) FROM public_equipment_rows').fetchone()[0],
                'japan_equipment': c.db.execute('SELECT count(*) FROM japan_equipment').fetchone()[0],
                'inspur_support_entries': c.db.execute('SELECT count(*) FROM inspur_support_entries').fetchone()[0],
                'ncc_equipment': c.db.execute('SELECT count(*) FROM ncc_equipment').fetchone()[0],
                'fcc_id_index': c.db.execute('SELECT count(*) FROM fcc_id_index').fetchone()[0],
                'fcc_company_rows': c.db.execute('SELECT count(*) FROM fcc_company_rows').fetchone()[0],
                'fcc_sitemap_pages': c.db.execute('SELECT count(*) FROM fcc_sitemap_pages').fetchone()[0],
                'fcc_sitemap_urls': c.db.execute('SELECT count(*) FROM fcc_sitemap_urls').fetchone()[0],
                'documents': c.db.execute('SELECT count(*) FROM documents').fetchone()[0],
                'document_pages': c.db.execute('SELECT count(*) FROM document_pages').fetchone()[0],
                'fcc_detail_pages': c.db.execute('SELECT count(*) FROM fcc_detail_pages').fetchone()[0],
                'fcc_detail_links': c.db.execute('SELECT count(*) FROM fcc_detail_links').fetchone()[0],
                'fcc_text_excerpts': c.db.execute('SELECT count(*) FROM fcc_text_excerpts').fetchone()[0],
            }
            from .lineage import TABLES
            actual.update({table: c.db.execute('SELECT count(*) FROM ' + table).fetchone()[0]
                           for table in TABLES})
            if actual != data['expected']:
                raise ValueError('Rebuilt counts differ from manifest: ' + json.dumps(actual))
            if [r[0] for r in c.db.execute('PRAGMA integrity_check')] != ['ok']:
                raise ValueError('Rebuilt database failed integrity check')
            if c.db.execute('PRAGMA foreign_key_check').fetchall():
                raise ValueError('Rebuilt database has invalid record references')
            # Check files again so concurrent edits cannot silently mix snapshots.
            _, rechecked = read_manifest(manifest)
            if rechecked != data:
                raise ValueError('Rebuild manifest changed during import')
        finally:
            c.db.close()
        os.link(staging, destination)
    return {'database': str(destination), 'devices': actual['devices'],
            'records': actual['records'], 'sources': len(actual['sources']),
            'public_equipment_rows': actual['public_equipment_rows'],
            'japan_equipment': actual['japan_equipment'],
            'inspur_support_entries': actual['inspur_support_entries'],
            'grantees': actual['grantees'],
            'equipment_register': actual['equipment_register'],
            'bahrain_equipment': actual['bahrain_equipment'],
            'anatel_equipment': actual['anatel_equipment'],
            'ntra_equipment': actual['ntra_equipment'],
            'tec_equipment': actual['tec_equipment'],
            'ieee_assignments': actual['ieee_assignments'],
            'pci_entries': actual['pci_entries'],
            'bsmi_certificates': actual['bsmi_certificates'],
            'bsmi_product_certificates': actual['bsmi_product_certificates'],
            'bsmi_voluntary_certificates': actual['bsmi_voluntary_certificates'],
            'bsmi_declarations': actual['bsmi_declarations'],
            'rra_equipment': actual['rra_equipment'],
            'energystar_servers': actual['energystar_servers'],
            'energystar_server_archives': actual['energystar_server_archives'],
            'energystar_datacenter': actual['energystar_datacenter'],
            'ised_equipment': actual['ised_equipment'],
            'ncc_equipment': actual['ncc_equipment'],
            'fcc_id_index': actual['fcc_id_index'],
            'fcc_company_rows': actual['fcc_company_rows'],
            'fcc_sitemap_pages': actual['fcc_sitemap_pages'],
            'fcc_sitemap_urls': actual['fcc_sitemap_urls'],
            'documents': actual['documents'],
            'document_pages': actual['document_pages'],
            'fcc_detail_pages': actual['fcc_detail_pages'],
            'fcc_detail_links': actual['fcc_detail_links'],
            'fcc_text_excerpts': actual['fcc_text_excerpts'],
            'verified_files': len(data['files'])}
