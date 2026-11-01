"""Independent stdlib HTML audit and deterministic source-row curation; offline.

Does not import the author's extraction or novelty modules. Every selected
original row/card becomes a distinct source record, including repeated models.
"""
from __future__ import annotations
import argparse
from collections import Counter
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sqlite3

ROOT = Path(__file__).resolve().parent
VOID = set('area base br col embed hr img input link meta param source track wbr'.split())
BLOCK = {'br', 'p', 'li', 'div', 'dt', 'dd', 'h2', 'h3'}
PARTNERS = {'Inventec', 'QCT', 'Quanta', 'Wiwynn', 'Inspur', 'MiTAC', 'MITAC', 'Wistron', 'PEGATRON', 'Foxconn', 'Ingrasys', 'Hyve', 'Supermicro'}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def dump(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))


class AuditHTML(HTMLParser):
    """Event-buffer parser independent of the original DOM/extraction code."""
    def __init__(self, raw):
        super().__init__(convert_charrefs=True)
        self.raw = raw
        self.html = raw.decode('utf-8', errors='strict')
        self.line_starts = [0] + [m.end() for m in re.finditer('\n', self.html)]
        self.stack = []
        self.buffers = []
        self.tables, self.cards = [], []
        self.table = self.row = self.card = None
        self.nearest_h2 = self.nearest_heading = None
        self.feed(self.html)
        self.close()
        if self.table or self.row or self.card or self.buffers:
            raise ValueError('unfinished selected HTML structure')

    def source_offset(self):
        line, col = self.getpos()
        return self.line_starts[line-1] + col

    def proof(self, start, end):
        line = self.html.count('\n', 0, start) + 1
        return {'char_start': start, 'char_end': end,
                'byte_start': len(self.html[:start].encode()), 'byte_end': len(self.html[:end].encode()),
                'line': line, 'fragment_sha256': sha(self.html[start:end].encode())}

    def handle_data(self, data):
        for buf in self.buffers:
            buf['raw_parts'].append(data)
            buf['parts'].append(data)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        for buf in self.buffers:
            if tag in BLOCK:
                buf['parts'].append(' ')
            if tag == 'a':
                buf['links'].append({'attributes': attrs})
            if tag == 'sup':
                buf['has_superscript'] = True
        depth = len(self.stack)
        if tag not in VOID:
            self.stack.append(tag)
        if tag == 'table':
            if self.table:
                raise ValueError('nested table')
            self.table = {'ordinal': len(self.tables)+1, 'attrs': attrs, 'start': self.source_offset(),
                          'heading': self.nearest_heading, 'rows': []}
        if tag == 'tr' and self.table:
            if self.row:
                raise ValueError('nested/unclosed row')
            self.row = {'ordinal': len(self.table['rows'])+1, 'attrs': attrs,
                        'start': self.source_offset(), 'cells': []}
        if tag == 'div' and 'swiper-slide' in attrs.get('class', '').split():
            if self.card:
                raise ValueError('nested product card')
            self.card = {'ordinal': len(self.cards)+1, 'family': self.nearest_h2,
                         'models': [], 'labels': [], 'values': [], 'fields': [],
                         'start': self.source_offset(), 'depth': depth, 'links': [], 'images': []}
        if self.card and tag == 'a':
            self.card['links'].append(attrs)
        if self.card and tag == 'img':
            self.card['images'].append(attrs)
        target = (tag in ('td', 'th') and self.row is not None or
                  tag in ('h1', 'h2', 'h3', 'h4') or
                  tag in ('dt', 'dd') and self.card is not None)
        if target:
            self.buffers.append({'tag': tag, 'depth': depth, 'attrs': attrs,
                                 'raw_parts': [], 'parts': [], 'links': [],
                                 'has_superscript': False, 'start': self.source_offset(),
                                 'card_ordinal': self.card['ordinal'] if self.card else None})

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if tag not in VOID:
            self.handle_endtag(tag)

    def handle_endtag(self, tag):
        if tag in self.stack:
            # HTML permits omitted tags outside selected structures; selected
            # buffers/rows must still terminate explicitly and consistently.
            depth = len(self.stack)-1-self.stack[::-1].index(tag)
            del self.stack[depth:]
        else:
            depth = -1
        for buf in self.buffers:
            if tag in BLOCK:
                buf['parts'].append(' ')
        matching = [b for b in self.buffers if b['tag'] == tag and b['depth'] == depth]
        for buf in matching:
            self.buffers.remove(buf)
            text = ' '.join(''.join(buf.pop('parts')).split())
            raw_text = ''.join(buf.pop('raw_parts'))
            buf.update(text=text, text_raw=raw_text,
                       proof=self.proof(buf.pop('start'), self.html.find('>', self.source_offset())+1))
            buf.pop('depth')
            if tag in ('td', 'th'):
                self.row['cells'].append(buf)
            if tag in ('h1', 'h2', 'h3', 'h4'):
                self.nearest_heading = text
            if tag == 'h2':
                self.nearest_h2 = text
            if self.card and buf['card_ordinal'] == self.card['ordinal']:
                if tag == 'h3':
                    self.card['models'].append(buf)
                elif tag == 'dt':
                    self.card['labels'].append(buf)
                elif tag == 'dd':
                    self.card['values'].append(buf)
        if tag == 'tr' and self.row:
            self.row['proof'] = self.proof(self.row.pop('start'), self.html.find('>', self.source_offset())+1)
            self.table['rows'].append(self.row)
            self.row = None
        if tag == 'table' and self.table:
            self.table['proof'] = self.proof(self.table.pop('start'), self.html.find('>', self.source_offset())+1)
            self.tables.append(self.table)
            self.table = None
        if self.card and tag == 'div' and depth == self.card['depth']:
            card = self.card
            card['proof'] = self.proof(card.pop('start'), self.html.find('>', self.source_offset())+1)
            if len(card['models']) != 1 or len(card['labels']) != len(card['values']):
                raise ValueError('ambiguous product card heading/fields')
            card['fields'] = [{'label': label, 'value': value} for label, value in zip(card['labels'], card['values'])]
            card.pop('depth')
            self.cards.append(card)
            self.card = None


def selected_table_rows(document):
    selected = []
    for table in document.tables:
        rows = table['rows']
        assert rows and all(c['tag'] == 'th' for c in rows[0]['cells'])
        headers = [c['text'] for c in rows[0]['cells']]
        assert headers[0] in ('Partner', 'Manufacturer')
        assert all(len(r['cells']) == len(headers) for r in rows)
        table_id = table['attrs'].get('id', 'ordinal-' + str(table['ordinal']))
        for row in rows[1:]:
            if row['cells'][0]['text'] not in PARTNERS:
                continue
            if not row['cells'][1]['text']:
                raise ValueError('empty selected identity cell')
            if any(c['has_superscript'] for c in row['cells'][:2]):
                raise ValueError('selected identity has unresolved superscript')
            if any(any(k in c['attrs'] for k in ('rowspan', 'colspan')) for c in row['cells']):
                raise ValueError('selected row has unresolved span')
            selected.append({'locator': {'table_id': table_id, 'row_ordinal': row['ordinal']},
                             'company': row['cells'][0]['text'], 'model': row['cells'][1]['text'],
                             'label': headers[0], 'headers': headers, 'heading': table['heading'],
                             'row': row, 'table_proof': table['proof']})
    return selected


def parse_source(raw, mitac=False):
    doc = AuditHTML(raw)
    if not mitac:
        return selected_table_rows(doc), doc
    rows = []
    for card in doc.cards:
        model = card['models'][0]['text']
        if not model or not any(i.get('alt') == model for i in card['images']):
            raise ValueError('card heading lacks matching literal image alt')
        labels = [f['label']['text'] for f in card['fields']]
        if labels not in (['CPUs', 'CPU Type', 'Form Factor', 'Storage Bays'],
                          ['CPUs', 'CPU Type', 'GPUs', 'Form Factor', 'Storage Bays'],
                          ['CPUs', 'CPU Type', 'Form Factor']):
            raise ValueError('unreviewed card field format')
        # A label's dd must occur after its dt and before the next dt.
        for i, field in enumerate(card['fields']):
            assert field['label']['proof']['char_end'] <= field['value']['proof']['char_start']
            if i+1 < len(card['fields']):
                assert field['value']['proof']['char_end'] <= card['fields'][i+1]['label']['proof']['char_start']
        rows.append({'locator': {'card_ordinal': card['ordinal']}, 'company': 'MiTAC',
                     'model': model, 'label': 'product listed on MiTAC Computing index', 'card': card})
    return rows, doc


def source_id(source):
    return {'nvidia-certified.html': 'nvidia-certified-system-index-2026-10-14',
            'redirect-nvidia-ngc-ready.pdf': 'nvidia-ngc-ready-system-index-2026-10-14',
            'mitac-general.html': 'mitac-general-purpose-index-2026-10-14',
            'mitac-ai.html': 'mitac-ai-server-index-2026-10-14',
            'mitac-multi.html': 'mitac-multi-node-index-2026-10-14'}[source['body_path']]


def build(selection, capture_root):
    sources, records, accepted = [], [], []
    for source in selection['sources']:
        raw = (capture_root / source['body_path']).read_bytes()
        if sha(raw) != source['body_sha256']:
            raise ValueError('source capture hash mismatch')
        mitac = source['body_path'].startswith('mitac-')
        rows, doc = parse_source(raw, mitac)
        expected = [r for r in selection['references'] if r['source_path'] == source['body_path']]
        keyed = {canonical(r['locator']): r for r in rows}
        if len(rows) != len(expected) or len(keyed) != len(rows):
            raise ValueError('selected source row coverage changed')
        sid = source_id(source)
        scope = ('Exact captured publisher table/card statements only; whole printed names remain separate. '
                 'No aliases, company-parent mapping, physical manufacture, deployment, stock, current '
                 'qualification/support guarantee, auction-unit configuration or licence entitlement inferred. '
                 'Each original row/card remains its own source record; duplicates and blank fields retained. ')
        if mitac:
            scope += ('MiTAC Computing title/host establish documentation_vendor only. Family headings '
                      'and / per node qualifiers are retained as printed; chassis/node equivalence is not inferred. '
                      'Only visible configuration cards in the captured indexes are selected; hydration-only '
                      'products are outside this selection.')
        else:
            scope += ('Partner means program_partner for this source row only. Historical Manufacturer '
                      'column is retained literally but assigned conservative listing_holder, not physical '
                      'manufacturer. Tested and Supported columns stay distinct; blank is not unsupported. '
                      'NGC-Ready AI Enterprise Compatible scope is conditional on NVIDIA V100 or T4 as the '
                      'captured article states; no general entitlement, installed GPU or current support inferred. '
                      'Nonidentity footnote markers are preserved without interpretation.')
        sources.append({'id': sid, 'url': source['url'], 'tier': 'vendor',
                        'license': 'Public factual product/program index; original upstream rights retained; local research only.',
                        'coverage': scope, 'snapshot': 'raw/' + source['body_path'],
                        'original_sha256': source['body_sha256'], 'captured_at_utc': source['finished_at_utc']})
        for reference in expected:
            row = keyed.get(canonical(reference['locator']))
            if row is None or (row['company'], row['model'], row['label']) != (
                    reference['company'], reference['model'], reference['label']):
                raise ValueError('proposal identity/role disagrees with original source row')
            accepted.append({'source_path': source['body_path'], **row})
            loc = canonical(row['locator'])
            key = ('card-' + str(row['locator']['card_ordinal']) if mitac else
                   row['locator']['table_id'] + ':row-' + str(row['locator']['row_ordinal']))
            facts = [['source_locator', loc], ['source_scope', scope]]
            if mitac:
                role, kind = 'documentation_vendor', 'server product index configuration'
                facts += [['product_card_raw_json', canonical(row['card'])],
                          ['product_family_heading_as_printed', row['card']['family']],
                          ['product_card_fragment_sha256', row['card']['proof']['fragment_sha256']]]
                facts += [['product_card_field_as_printed', canonical({'label': f['label']['text'], 'value': f['value']['text']}) + ' [' + loc + ']']
                          for f in row['card']['fields']]
            else:
                role = 'program_partner' if row['label'] == 'Partner' else 'listing_holder'
                kind = 'NVIDIA system program listing'
                facts += [['nvidia_table_row_raw_json', canonical(row['row'])],
                          ['nvidia_table_headers_as_printed', canonical(row['headers'])],
                          ['nvidia_table_heading_as_printed', row['heading']],
                          ['nvidia_company_column_label_as_printed', row['label']],
                          ['nvidia_row_fragment_sha256', row['row']['proof']['fragment_sha256']]]
                facts += [['nvidia_table_cell_as_printed', canonical({'column': header, 'value': cell['text']}) + ' [' + loc + ']']
                          for header, cell in zip(row['headers'], row['row']['cells'])]
            records.append({'source': sid, 'key': key, 'brand': row['company'], 'model': row['model'],
                            'title': row['company'] + ' ' + row['model'], 'kind': kind,
                            'affiliations': [[row['company'], role]], 'identifiers': [],
                            'hyperscaler': [], 'facts': facts})
    return {'sources': sources, 'devices': records}, accepted


def audit(original, destination):
    proposal = json.loads((original / 'curated-proposal.json').read_text())
    discovery = json.loads((original / 'discovery-rows.json').read_text())
    bulk = json.loads((original / 'bulk-extracted.json').read_text())
    receipts = [json.loads(line) for line in (original / 'requests.jsonl').read_text().splitlines()]
    hashes = []
    for receipt in receipts:
        for label, key in [('body', 'body_path'), ('header', 'header_path')]:
            data = (original / receipt[key]).read_bytes()
            assert sha(data) == receipt[label + '_sha256']
            hashes.append({'path': receipt[key], 'sha256': sha(data), 'bytes': len(data)})
    refs = []
    for pair in proposal['candidates']:
        for ref in pair['references']:
            row = discovery[ref['discovery_row_index']]
            assert (pair['printed_company'], pair['printed_model']) == (row['partner'], row['model'])
            assert ref['source_url'] == row['source_url'] and ref['source_sha256'] == row['source_sha256']
            assert ref['source_path'] == row['source_path'] and ref['locator'] == row['locator']
            assert not row.get('has_spans') and not row.get('has_identity_superscripts')
            refs.append({'source_path': ref['source_path'], 'source_url': ref['source_url'],
                         'source_sha256': ref['source_sha256'], 'locator': ref['locator'],
                         'company': pair['printed_company'], 'model': pair['printed_model'],
                         'label': ref['role_label_as_printed'], 'discovery_row_index': ref['discovery_row_index']})
    assert len(proposal['candidates']) == 154 and len(refs) == 214 and not proposal['held_rows']
    assert len({r['discovery_row_index'] for r in refs}) == 214
    source_paths = {r['source_path'] for r in refs}
    sources = [r for r in receipts if r['body_path'] in source_paths]
    assert len(sources) == 5 and all(r['status'] == 200 and not r['is_policy'] for r in sources)
    selection = {'sources': sources, 'references': refs,
                 'proposal_sha256': sha((original / 'curated-proposal.json').read_bytes()),
                 'discovery_sha256': sha((original / 'discovery-rows.json').read_bytes())}
    (destination / 'raw').mkdir(parents=True, exist_ok=True)
    for source in sources:
        for key in ('body_path', 'header_path'):
            dest = destination / 'raw' / source[key]
            data = (original / source[key]).read_bytes()
            if dest.exists() and dest.read_bytes() != data:
                raise ValueError('refuse to replace original copied capture')
            dest.write_bytes(data)
    candidate, accepted = build(selection, destination / 'raw')
    # Independently compare every selected raw cell/card field to the author's
    # extracted view, including all blanks and complete raw decoded text.
    all_source = {s['source']['path']: s for s in bulk['sources']}
    compared = 0
    for row in accepted:
        source = all_source[row['source_path']]
        if 'row' in row:
            tables = {t['table_id']: t for t in source['tables']}
            table = tables[row['locator']['table_id']]
            author_row = next(r for r in table['rows'] if r['row_ordinal'] == row['locator']['row_ordinal'])
            assert row['headers'] == table['headers'][0]
            assert [(c['text'], c['text_raw']) for c in row['row']['cells']] == [(c['text'], c['text_raw']) for c in author_row['cells']]
            assert row['row']['proof']['fragment_sha256'] == author_row['provenance']['fragment_sha256']
        else:
            author_card = source['cards'][row['locator']['card_ordinal']-1]
            assert row['model'] == author_card['model'] and row['card']['family'] == author_card['family_heading']
            assert [(f['label']['text'], f['value']['text']) for f in row['card']['fields']] == [(f['label']['text'], f['value']['text']) for f in author_card['fields']]
            assert row['card']['proof']['fragment_sha256'] == author_card['provenance']['fragment_sha256']
        compared += 1
    dump(destination / 'selection.json', selection)
    dump(destination / 'candidate.json', candidate)
    dump(destination / 'accepted-rows.json', accepted)
    report = {'input_capture_files_hashed': hashes, 'independent_parser': 'stdlib HTMLParser event buffers; original extractor not imported',
              'pairs_verified': len(proposal['candidates']), 'references_verified': compared,
              'source_count': len(sources), 'records': len(candidate['devices']),
              'candidate_facts': sum(len(r['facts']) for r in candidate['devices']),
              'roles': dict(Counter(role for r in candidate['devices'] for _, role in r['affiliations'])),
              'amd_imported': False, 'span_or_identity_superscript_in_selected_rows': False,
              'blank_table_cells_retained': sum(c['text'] == '' for r in accepted if 'row' in r for c in r['row']['cells']),
              'proposal_sha256': selection['proposal_sha256'], 'discovery_sha256': selection['discovery_sha256']}
    dump(destination / 'review-checks.json', report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--audit-source', type=Path)
    args = parser.parse_args()
    if args.audit_source:
        result = audit(args.audit_source, ROOT)
    else:
        candidate, rows = build(json.loads((ROOT / 'selection.json').read_text()), ROOT / 'raw')
        dump(ROOT / 'candidate.json', candidate)
        dump(ROOT / 'accepted-rows.json', rows)
        result = {'records': len(candidate['devices']), 'sources': len(candidate['sources'])}
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
