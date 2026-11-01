"""Independent offline source projection and SQLite equivalence review."""
from collections import Counter
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
from urllib.parse import parse_qsl, urljoin, urlsplit, urlunsplit
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SOURCE = HERE.parent / 'vendor-source-expansion'
VOID = set('area base br col embed hr img input link meta param source track wbr'.split())


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def normal(text):
    return ' '.join(text.split())


def endpoint(url):
    p = urlsplit(url)
    return urlunsplit((p.scheme.lower(), p.netloc.lower(), p.path, '', ''))


class ProjectionParser(HTMLParser):
    """Event buffers bind anchors to original nearest cards/tables, independently."""
    def __init__(self, raw):
        super().__init__(convert_charrefs=True)
        self.stack, self.buffers, self.anchors = [], [], []
        self.ordinal = 0
        self.group = ''
        self.feed(raw.decode('utf-8'))
        self.close()
        if self.buffers:
            raise ValueError('unclosed projection buffer')

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        depth = len(self.stack)
        classes = attrs.get('class', '').split()
        frame = {'tag': tag, 'attrs': attrs, 'card': None}
        if tag == 'table':
            frame['group'] = self.group
        if 'well' in classes and 'c2a3' in classes:
            frame['card'] = {'headings': [], 'metadata': []}
        if tag == 'a':
            self.ordinal += 1
            table = next((n for n in reversed(self.stack) if n['tag'] == 'table'), None)
            card = next((n['card'] for n in reversed(self.stack) if n['card'] is not None), None)
            ancestor_classes = [c for n in reversed(self.stack) for c in n['attrs'].get('class', '').split()]
            self.buffers.append({'tag': tag, 'depth': depth, 'kind': 'anchor', 'parts': [],
                                 'attrs': attrs, 'ordinal': self.ordinal, 'line': self.getpos()[0],
                                 'ancestor_classes': ancestor_classes, 'card': card,
                                 'table_id': table['attrs'].get('id', '') if table else None,
                                 'group': table['group'] if table else None})
        if tag == 'h3' and 'Group' in classes:
            self.buffers.append({'tag': tag, 'depth': depth, 'kind': 'group', 'parts': []})
        card = next((n['card'] for n in reversed(self.stack) if n['card'] is not None), None)
        if card is not None and ((tag == 'h3' and 'media-heading' in classes) or 'text-muted' in classes):
            self.buffers.append({'tag': tag, 'depth': depth, 'kind': 'card-heading' if tag == 'h3' else 'card-metadata',
                                 'parts': [], 'card': card})
        if tag not in VOID:
            self.stack.append(frame)

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if tag not in VOID:
            self.handle_endtag(tag)

    def handle_data(self, text):
        for b in self.buffers:
            b['parts'].append(text)

    def handle_endtag(self, tag):
        indices = [i for i,n in enumerate(self.stack) if n['tag'] == tag]
        if not indices:
            return
        depth = indices[-1]
        for b in list(self.buffers):
            if b['tag'] != tag or b['depth'] != depth:
                continue
            self.buffers.remove(b)
            b['text'] = ''.join(b.pop('parts'))
            if b['kind'] == 'anchor':
                self.anchors.append(b)
            elif b['kind'] == 'group':
                self.group = b['text']
            elif b['kind'] == 'card-heading':
                b['card']['headings'].append(b['text'])
            elif b['kind'] == 'card-metadata':
                b['card']['metadata'].append(b['text'])
        del self.stack[depth:]


def main():
    config = json.loads((SOURCE / 'inputs-ready.json').read_text())
    dataset = json.loads((SOURCE / 'discovery-ready.json').read_text())
    pinned = {name: sha(SOURCE / name) for name in ('inputs-ready.json','discovery-ready.json','discovery.sqlite','offline_parser.py','build_discovery_db.py','verify_offline.py','integration-contract.md')}
    live_before = sha(ROOT / 'data/devices.sqlite')
    manifest_before = sha(ROOT / 'data/import-manifest.json')
    capture_checks, dependency_checks, projections = [], [], []
    for dependency in config['dependencies']:
        actual = sha(ROOT / dependency['path'])
        assert actual == dependency['sha256']
        dependency_checks.append({'path': dependency['path'], 'sha256': actual})
    seen = Counter()
    actual_by_key = {(r['capture_path'], r['row_number']): r for r in dataset['rows']}
    assert len(actual_by_key) == len(dataset['rows']) == 889
    for source in config['sources']:
        for capture in source['inputs']:
            path = (ROOT / capture['path']).resolve()
            assert path.is_relative_to(ROOT.resolve()) and capture['status'] == 200
            actual_hash = sha(path)
            assert actual_hash == capture['sha256']
            capture_checks.append({'path': capture['path'], 'sha256': actual_hash})
            if capture['format'] == 'xml':
                doc = ET.fromstring(path.read_bytes())
                assert doc.tag.endswith('urlset')
                items = [{'ordinal': i, 'url': item.find('{*}loc').text,
                          'lastmod': item.find('{*}lastmod').text if item.find('{*}lastmod') is not None else None}
                         for i,item in enumerate(list(doc),1)]
            else:
                items = []
                for anchor in ProjectionParser(path.read_bytes()).anchors:
                    href = anchor['attrs'].get('href', '')
                    if not href:
                        continue
                    url = urljoin(capture['url'], href)
                    parts = urlsplit(url)
                    classes = anchor['attrs'].get('class', '').split()
                    if parts.scheme not in ('http','https'):
                        continue
                    if source.get('same_host') and parts.netloc != urlsplit(capture['url']).netloc:
                        continue
                    if source.get('path_pattern') and not re.search(source['path_pattern'], parts.path):
                        continue
                    if source.get('required_anchor_class') and source['required_anchor_class'] not in classes:
                        continue
                    if source.get('required_ancestor_class') and source['required_ancestor_class'] not in anchor['ancestor_classes']:
                        continue
                    items.append({'ordinal': anchor['ordinal'], 'url': url, 'href': href, 'anchor': anchor})
            for item in items:
                key = (capture['path'], item['ordinal'])
                row = actual_by_key[key]
                assert row['capture_sha256'] == actual_hash and row['source'] == source['id']
                assert row['url'] == item['url'] and row['endpoint_comparison_key'] == endpoint(item['url'])
                if 'anchor' in item:
                    anchor = item['anchor']
                    assert row['href_raw'] == item['href']
                    assert row['printed_label_raw'] == anchor['text'] and row['printed_label'] == normal(anchor['text'])
                    assert row['source_line'] == anchor['line']
                    if anchor['table_id'] is not None:
                        assert row['table_id'] == anchor['table_id']
                        assert row['section_label_raw'] == anchor['group']
                    if anchor['card'] is not None:
                        card = anchor['card']
                        assert row['document_heading_raw'] == (card['headings'][0] if card['headings'] else '')
                        assert row['document_metadata_raw'] == (card['metadata'][0] if card['metadata'] else '')
                    assert row['query_pairs'] == [list(pair) for pair in parse_qsl(urlsplit(item['url']).query, keep_blank_values=True)]
                else:
                    assert row['loc_raw'] == item['url'] and row['lastmod_raw'] == item['lastmod']
                assert row['evidence_class'] == 'discovery_only'
                assert not any(row[k] for k in ('identity_assertion','manufacturer_assertion','deployment_assertion'))
                assert row['source_role'] == source['source_role']
                seen[key] += 1
            projections.append({'source': source['id'], 'capture_path': capture['path'], 'rows_verified': len(items)})
    assert len(capture_checks) == 243 and len(seen) == 889 and all(v == 1 for v in seen.values())
    assert set(seen) == set(actual_by_key)
    restricted = [r for r in dataset['rows'] if r['restrictions']]
    assert len(restricted) == 1 and 'dcs203-as7326-56x-ds-r09' in restricted[0]['url']
    assert restricted[0]['document_heading'] == 'DCS203 (AS7326-56X) DS R10'
    assert restricted[0]['capture_scheduling'] == 'excluded_known_restriction_or_missing'
    assert sum(bool(r['revision_label_url_disagreement']) for r in dataset['rows']) == 40
    # Compare every SQL row projection and complete raw JSON to the pinned input.
    with sqlite3.connect((SOURCE / 'discovery.sqlite').resolve().as_uri()+'?mode=ro', uri=True) as db:
        db.row_factory = sqlite3.Row
        db.execute('PRAGMA query_only=ON')
        for number, record in enumerate(db.execute('SELECT * FROM links ORDER BY id')):
            row = dataset['rows'][number]
            assert json.loads(record['raw_json']) == row
            expected = (row['capture_path'],row['source'],row['source_role'],row['row_number'],row['source_url'],row['printed_label'],row.get('document_heading',''),row.get('document_metadata',''),row.get('section_label',''),row['url'],row['endpoint_comparison_key'],int(bool(row['restrictions'])),int(bool(row['revision_label_url_disagreement'])),row['evidence_class'])
            columns = ('capture_path','source','source_role','source_row','source_url','label','document_heading','document_metadata','section_label','raw_url','endpoint','restricted','revision_disagreement','evidence_class')
            assert tuple(record[k] for k in columns) == expected
        assert number+1 == 889
        assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert not db.execute('PRAGMA foreign_key_check').fetchall()
        assert db.execute('SELECT count(*) FROM link_search').fetchone()[0] == 889
    # Rebuild independently to a NEW owned path, using the delivered builder.
    rebuilt = HERE / 'reconstructed.sqlite'
    if rebuilt.exists():
        raise FileExistsError('choose a new output rather than replacing the previous proof')
    result = subprocess.run([sys.executable,'-B',str(SOURCE/'build_discovery_db.py'),'build','--input',str(SOURCE/'discovery-ready.json'),'--output',str(rebuilt)],capture_output=True,text=True)
    assert result.returncode == 0, result.stderr
    with sqlite3.connect(rebuilt.resolve().as_uri()+'?mode=ro',uri=True) as db:
        db.execute('ATTACH DATABASE ? AS original', ((SOURCE/'discovery.sqlite').resolve().as_uri()+'?mode=ro',))
        for table in ('datasets','captures','links','link_search'):
            assert db.execute(f'SELECT * FROM main.{table} EXCEPT SELECT * FROM original.{table}').fetchall() == []
            assert db.execute(f'SELECT * FROM original.{table} EXCEPT SELECT * FROM main.{table}').fetchall() == []
    assert {name:sha(SOURCE/name) for name in pinned} == pinned
    assert sha(ROOT/'data/devices.sqlite') == live_before
    assert sha(ROOT/'data/import-manifest.json') == manifest_before
    report = {'result':'pass','reviewer':'/root/network_bulk_full','network_calls':0,
              'permissions':{'filesystem':'danger-full-access','network_enabled':True,'approval_policy':'never'},
              'input_artifact_sha256':pinned,'capture_hashes':capture_checks,'dependency_hashes':dependency_checks,
              'capture_count':len(capture_checks),'independent_full_row_projections':len(seen),
              'rows_by_source':dict(Counter(r['source'] for r in dataset['rows'])),
              'capture_projections':projections,'sql_complete_raw_json_and_projections_verified':889,
              'reconstructed_path':str(rebuilt.relative_to(ROOT)),'reconstructed_sha256':sha(rebuilt),
              'logical_db_tables_equal':['datasets','captures','links','link_search'],
              'restricted_row':{'heading':restricted[0]['document_heading'],'url':restricted[0]['url'],'capture_path':restricted[0]['capture_path']},
              'revision_disagreements_retained':40,'catalog_sha256_before_after':live_before,
              'manifest_sha256_before_after':manifest_before,'source_artifacts_unchanged':True}
    (HERE/'audit.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k not in ('capture_hashes','dependency_hashes','capture_projections')},indent=2))


if __name__ == '__main__':
    main()
