"""Offline finite observations of successful saved SPEC pages, not a curated candidate."""
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import sys

sys.dont_write_bytecode = True

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
PARSER = ROOT / 'research/batches/discovery-2026-10-11/spec/scripts/jbbparse.py'
LABELS = {'Name', 'Vendor', 'Model', 'Available', 'Form Factor', 'CPU Name', 'CPU Characteristics',
          'CPU Frequency (MHz)', 'Nodes Per System', 'Number of Systems', 'Chips Per System',
          'Cores Per System', 'Cores Per Chip', 'Threads Per System', 'Memory Amount (GB)',
          '# and size of DIMM(s)', 'Memory Details', 'Power Supply Quantity and Rating (W)',
          'Power Supply Details', 'Cabinet/Housing/Enclosure', 'Shared Description', 'Shared Comment'}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    assert (HERE / 'capture-outcomes.json').exists(), 'wait for finite acquisition to finish'
    plan = json.loads((HERE / 'plan.json').read_text())
    requests = {r['url']: r for r in map(json.loads, (HERE / 'requests.jsonl').read_text().splitlines())}
    spec = importlib.util.spec_from_file_location('saved_jbb_cell_parser', PARSER)
    parser = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(parser)
    pages = []
    for selected in plan['sources']:
        request = requests.get(selected['url'])
        if not request or request['status'] != 200 or request['curl_returncode'] or request['login_marker_detected']:
            continue
        path = HERE / request['body_path']
        assert sha(path) == request['body_sha256']
        base = {'url': selected['url'], 'benchmark': selected['benchmark'],
                'captured_at_utc': request['finished_at_utc'], 'body_path': request['body_path'],
                'capture_sha256': request['body_sha256'], 'saved_index_row': selected['saved_index_row'],
                'scope': 'Historical submitted test configuration only. No identity routing, role, manufacture, deployment, current support or hardware-capability decision.'}
        if selected['benchmark'] == 'jbb2015':
            parsed = parser.parse(path)
            rows = parsed.rows
            blocks = []
            for i, row in enumerate(rows):
                if len(row) != 1 or not re.fullmatch(r'Hardware hw_\d+', row[0]):
                    continue
                fields = []
                for j in range(i + 1, len(rows)):
                    following = rows[j]
                    if len(following) == 1:
                        break
                    if len(following) == 2 and following[0] in LABELS:
                        fields.append({'label_as_printed': following[0], 'value_as_printed_cell_text': following[1],
                                       'locator': {'table_row_index_zero_based': j, 'section': row[0]},
                                       'raw_cell_row': following,
                                       'literal_None_means_source_omission': following[1] == 'None'})
                blocks.append({'section_as_printed': row[0], 'fields': fields})
            assert blocks and blocks[0]['section_as_printed'] == 'Hardware hw_1'
            header = [{'row_index_zero_based': i, 'raw_cells_whitespace_normalized': r}
                      for i, r in enumerate(rows[:3])]
            base.update({'document_title': parsed.title, 'header_rows': header, 'hardware_blocks': blocks,
                         'text_method': 'Saved stdlib HTML cell parser; cell whitespace normalized only, raw HTML unchanged.',
                         'block_boundary': 'Every found Hardware hw_N block retained separately; hw_2 fields never assigned to hw_1.',
                         'benchmark_metrics_boundary': 'Header max-jOPS/critical-jOPS are benchmark throughput for the printed mode, not hardware specifications; modes are not cross-comparable.'})
        else:
            lines = path.read_text(encoding='utf-8', errors='strict').splitlines()
            sections = []
            for i, line in enumerate(lines):
                if line.strip() not in ('HARDWARE', 'POWER-RELEVANT HARDWARE'):
                    continue
                if i + 1 >= len(lines) or set(lines[i + 1].strip()) != {'-'}:
                    continue
                end = len(lines)
                for j in range(i + 2, len(lines) - 1):
                    if re.fullmatch(r'\s*[A-Z][A-Z -]+\s*', lines[j]) and set(lines[j + 1].strip()) == {'-'}:
                        end = j
                        break
                fields = []
                for j in range(i + 2, end):
                    match = re.match(r'^\s*([^:]{1,24}):\s?(.*)$', lines[j])
                    if match and len(lines[j]) - len(lines[j].lstrip()) < 22:
                        fields.append({'label_as_printed': match[1].strip(), 'value_lines_as_printed': [match[2]],
                                       'lines_1based': [j + 1], 'raw_lines': [lines[j]]})
                    elif fields and lines[j].strip() and len(lines[j]) - len(lines[j].lstrip()) >= 22:
                        fields[-1]['value_lines_as_printed'].append(lines[j])
                        fields[-1]['lines_1based'].append(j + 1)
                        fields[-1]['raw_lines'].append(lines[j])
                sections.append({'section_as_printed': line.strip(), 'fields': fields,
                                 'start_line_1based': i + 1, 'raw_section_lines': lines[i:end]})
            assert any(s['section_as_printed'] == 'HARDWARE' for s in sections)
            base.update({'header_lines': [{'line_1based': i + 1, 'raw_line': line} for i, line in enumerate(lines[:7])],
                         'hardware_sections': sections,
                         'printed_system_boundary': 'Header vendor/system text and saved index hw_model preserved; no vendor splitting, CPU suffix removal or identity normalization.',
                         'power_section_present': any(s['section_as_printed'] == 'POWER-RELEVANT HARDWARE' for s in sections),
                         'benchmark_metrics_boundary': 'Benchmark rate/speed scores are not hardware capability facts; not selected as hardware observations.'})
        pages.append(base)
    result = {'status': 'finite research observations only; no candidate or role decisions', 'observed_pages': pages,
              'jbb_parser_path': str(PARSER.relative_to(ROOT)), 'jbb_parser_sha256': sha(PARSER),
              'no_normalization_of_vendor_or_system_names': True,
              'rights': 'Original SPEC and submitter rights retained; local factual research only.'}
    with (HERE / 'observations.json').open('x') as handle:
        json.dump(result, handle, ensure_ascii=False, indent=2)
        handle.write('\n')
    print('observed', len(pages), 'saved pages; no candidate or import')


if __name__ == '__main__':
    main()
