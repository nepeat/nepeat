"""Saved-input campaign adapters for the RRA/ISED pilot (c2).

Each adapter wraps the established source validator and returns exact raw
occurrences. Snapshot ordinals are occurrence selectors, never claim identities.
Nothing here imports into the catalog, reads the network or guesses aliases.
"""
from __future__ import annotations
import csv
import json
from pathlib import Path, PurePosixPath

from .artifacts import canonical, digest

# Validator failures that describe the saved bytes. Other exceptions (adapter
# bugs, I/O) stop the run with the job pending instead of becoming source outcomes.
INPUT_ERRORS = (ValueError, csv.Error)


def logical(value):
    return digest(canonical(value))


def require_shape(value, expected, label):
    """Reject saved JSON shapes explicitly; never translate parser/programming errors."""
    if type(value) is not expected:
        raise ValueError('Invalid saved-input shape: ' + label + ' must be ' + expected.__name__)
    return value


def string_fields(value, fields, label):
    require_shape(value, dict, label)
    for field in fields:
        require_shape(value.get(field), str, label + '.' + field)


def dependency_shape(entry, label):
    string_fields(entry, ('path', 'sha256'), label)


def company_evidence_shape(root, entry, seen=None, allowed_wrappers=None):
    """Check shapes used by supported RRA evidence readers, not their claim semantics."""
    from .documents import dependency
    dependency_shape(entry, 'company_evidence')
    seen = set() if seen is None else seen
    if entry['path'] in seen:
        raise ValueError('Invalid saved-input shape: cyclic company evidence')
    seen = seen | {entry['path']}
    data = require_shape(json.loads(dependency(root, entry)), dict, 'company evidence document')
    kind = data.get('kind')
    # The semantic readers support composite -> historical -> leaf only.
    # Never descend through arbitrary saved wrapper chains before they reject it.
    wrappers = ('celestica-historical-company-names', 'rra-composite-manufacturer-cells')
    if allowed_wrappers is not None and kind in wrappers and kind not in allowed_wrappers:
        raise ValueError('Invalid saved-input shape: unsupported company evidence nesting')
    if kind == 'celestica-historical-company-names':
        company_evidence_shape(root, data.get('base'), seen, allowed_wrappers=())
        exhibits = require_shape(data.get('exhibits'), dict, 'company evidence.exhibits')
        for exhibit in exhibits.values():
            company_evidence_shape(root, exhibit, seen, allowed_wrappers=())
    else:
        string_fields(data, ('reviewed_on',), 'company evidence')
    if kind == 'rra-composite-manufacturer-cells':
        company_evidence_shape(root, data.get('base'), seen,
                               allowed_wrappers=('celestica-historical-company-names',))
        entries = require_shape(data.get('entries'), list, 'company evidence.entries')
        for item in entries:
            string_fields(item, ('certificate_number', 'detail_sha256', 'detail_url', 'query_maker',
                                 'separator'), 'composite entry')
            cell = require_shape(item.get('cell'), dict, 'composite entry.cell')
            string_fields(cell, ('field', 'text', 'text_sha256'), 'composite entry.cell')
            raw = require_shape(cell.get('raw_html_cell_bytes'), dict, 'composite entry.cell.raw_html_cell_bytes')
            string_fields(raw, ('sha256',), 'composite raw bytes')
            for key in ('offset', 'length'):
                require_shape(raw.get(key), int, 'composite raw bytes.' + key)
            string_fields(item.get('unlabelled_parenthetical'), ('text',), 'composite parenthetical')
            for part in require_shape(item.get('parts'), list, 'composite entry.parts'):
                string_fields(part, ('text',), 'composite part')
                if 'target' not in part:
                    raise ValueError('Invalid saved-input shape: composite part.target is required')
                for key in ('role', 'target'):
                    if part.get(key) is not None:
                        require_shape(part[key], str, 'composite part.' + key)


def rra_input_shape(manifest):
    data = require_shape(json.loads(manifest.read_text()), dict, 'RRA bundle')
    for field in ('form', 'csv_before', 'csv_after'):
        dependency_shape(data.get(field), 'RRA bundle.' + field)
    for field in ('first_result_page', 'empty_result'):
        if field in data:
            dependency_shape(data[field], 'RRA bundle.' + field)
    query = require_shape(data.get('query'), dict, 'RRA bundle.query')
    string_fields(query, ('category', 'fromdate', 'todate', 'firm', 'equip', 'model_no',
                          'app_no', 'maker', 'nation'), 'RRA bundle.query')
    for entry in require_shape(data.get('details'), list, 'RRA bundle.details'):
        dependency_shape(entry, 'RRA detail')
        string_fields(entry, ('certificate', 'url'), 'RRA detail')
    if data.get('company_evidence') is not None:
        company_evidence_shape(manifest.parent, data['company_evidence'])


def ised_input_shape(manifest):
    data = require_shape(json.loads(manifest.read_text()), dict, 'ISED capture')
    string_fields(data, ('company_query',), 'ISED capture')
    for entry in require_shape(data.get('responses'), list, 'ISED capture.responses'):
        dependency_shape(entry, 'ISED response')
        string_fields(entry, ('url', 'final_url', 'method'), 'ISED response')
    if data.get('schema_version') == 2:
        for page in require_shape(data.get('result_pages'), list, 'ISED capture.result_pages'):
            require_shape(page, str, 'ISED result page')
    return data


class SavedAdapter:
    source = None
    version = '1'
    modules = ()

    def records(self, manifest, relative):
        raise NotImplementedError

    def compare(self, output, expectations):
        """Pinned c0 expectations are checks, never inputs to the derivation."""
        if not expectations:
            return dict(status='not_pinned', mismatches=[])
        mismatches = [key for key, actual in self.pinned_values(output).items()
                      if key in expectations and expectations[key] != actual]
        return dict(status='matches_pinned' if not mismatches else 'differs_from_pinned', mismatches=sorted(mismatches))


class RraSavedAdapter(SavedAdapter):
    source = 'rra'
    modules = ('devicehunt/campaign_sources.py', 'devicehunt/rra.py', 'devicehunt/rra_company_names.py',
               'devicehunt/catalog.py')

    def records(self, manifest, relative):
        from . import rra, rra_company_names
        rra_input_shape(manifest)
        data, ready = rra.validate(manifest)
        reviewed = rra_company_names.load(manifest.parent, data.get('company_evidence'))
        claims = rra.composite_claims(manifest.parent, data, ready)
        originals, occurrences = [], []
        for n, (cells, entry, row) in enumerate(ready, 1):
            original = {'csv_cells': cells, 'detail_entry': entry, 'labelled_fields': row}
            originals.append(original)
            eligible = bool(rra.claim_target(row, reviewed, claims))
            keys = [row['certificate_number'] + ':' + model for _, model in rra.models(row)] if eligible else []
            occurrences.append(dict(
                kind='rra-certificate-row', ordinal=n, certificate=row['certificate_number'],
                selector=dict(csv_row=n, detail_path=str(PurePosixPath(relative).parent / entry['path'])),
                logical_sha256=logical(original), projected_source_keys=keys, eligible_manufacturer=eligible,
                manufacturer=row['manufacturer'], applicant=row['applicant']))
        return dict(
            outcome='explicit_zero' if not ready else 'validated_nonempty',
            scope=dict(exact_query=data['query'], comparison_mode=data.get('csv_comparison', 'byte_identical')),
            zero_proof=data.get('empty_result'),
            query_records=len(ready), upstream_total=None,
            stats=dict(csv_rows_before=len(ready), csv_rows_after=len(ready), detail_pages=len(ready),
                       eligible_manufacturer_rows=sum(o['eligible_manufacturer'] for o in occurrences),
                       model_assertions=sum(len(o['projected_source_keys']) for o in occurrences)),
            record_sets=dict(validated_original_records_sha256=logical(originals)),
            occurrences=occurrences)

    def pinned_values(self, output):
        return dict(output['stats'], comparison_mode=output['scope']['comparison_mode'],
                    validated_original_records_sha256=output['record_sets']['validated_original_records_sha256'],
                    occurrences=[dict(certificate=o['certificate'], detail_path=o['selector']['detail_path'],
                                      logical_sha256=o['logical_sha256'], ordinal=o['ordinal'],
                                      projected_source_keys=o['projected_source_keys'])
                                 for o in output['occurrences']])


class IsedSavedAdapter(SavedAdapter):
    source = 'ised'
    modules = ('devicehunt/campaign_sources.py', 'devicehunt/ised.py', 'devicehunt/catalog.py')

    def records(self, manifest, relative):
        from . import ised
        from .catalog import target_for
        data = ised_input_shape(manifest)
        if data.get('schema_version') == 1:
            number, company, exported = ised.validate_capture(manifest)
            rows = []
            for entry in data['responses']:
                text = (manifest.parent / entry['path']).read_text(encoding='utf-8-sig')
                if 'id="equipmentSearchResultTable"' in text:
                    _, _, labelled = ised.html_rows(text)
                    rows = [{'group_number': number, 'company': company, 'cells': cells} for cells in labelled]
                    break
            query = data['company_query']
            stats = {'query_records': len(rows), 'csv_rows': len(exported),
                     'html_without_exact_csv': 0, 'unmatched_csv_rows': 0}
            comparison = 'exact_html_csv_identity_multiset'
            matched = set(range(1, len(exported) + 1))
        else:
            query, rows, exported, stats = ised.validate_paged_capture(manifest)
            comparison = 'contiguous_html_ranges_stable_totals_exact_csv_matches_only'
            matched = {m['row_number'] for row in rows for m in row['csv_matches']}
        target = target_for(data['company_query'])
        occurrences = [dict(kind='ised-html-row', ordinal=n, certificate=row['cells'][4], hvin=row['cells'][0],
                            selector=dict(html_row=n), logical_sha256=logical(row), company=row['company'],
                            target_company=target_for(row['company']) == target,
                            exact_csv_ordinals=[m['row_number'] for m in row.get('csv_matches', [])]
                            if data['schema_version'] == 2 else None)
                       for n, row in enumerate(rows, 1)]
        occurrences += [dict(kind='ised-csv-row', ordinal=n, certificate=row[7], hvin=row[2],
                             selector=dict(csv_row=n), logical_sha256=logical(row), matches_html_identity=n in matched)
                        for n, row in enumerate(exported, 1)]
        return dict(
            outcome='explicit_zero' if not rows else 'validated_nonempty',
            scope=dict(exact_company_query=query, comparison_mode=comparison,
                       saved_session_context='unknown legacy session; offline replay only'),
            zero_proof=data.get('result_pages') if not rows else None,
            query_records=stats['query_records'], upstream_total=None,
            stats=stats,
            record_sets=dict(validated_html_records_sha256=logical(rows), validated_csv_records_sha256=logical(exported)),
            occurrences=occurrences)

    def pinned_values(self, output):
        html = [o for o in output['occurrences'] if o['kind'] == 'ised-html-row']
        rows = [o for o in output['occurrences'] if o['kind'] == 'ised-csv-row']
        return dict(output['stats'], comparison_mode=output['scope']['comparison_mode'], **output['record_sets'],
                    html_occurrences_logical=[o['logical_sha256'] for o in html],
                    csv_occurrences_logical=[o['logical_sha256'] for o in rows])

    def compare(self, output, expectations):
        if expectations and 'html_occurrences' in expectations:
            expectations = dict(expectations,
                                html_occurrences_logical=[o['logical_sha256'] for o in expectations['html_occurrences']],
                                csv_occurrences_logical=[o['logical_sha256'] for o in expectations['csv_occurrences']])
        return super().compare(output, expectations)


ADAPTERS = {adapter.source: adapter for adapter in (RraSavedAdapter(), IsedSavedAdapter())}


def module_closure(names):
    """Package modules reachable through local imports from `names` (sorted paths)."""
    import ast
    base = Path(__file__).resolve().parents[1]
    seen, todo = set(), list(names)
    while todo:
        name = todo.pop()
        if name in seen:
            continue
        seen.add(name)
        tree = ast.parse((base / name).read_text(), name)
        for node in ast.walk(tree):
            modules = []
            if isinstance(node, ast.ImportFrom) and node.level == 1:
                modules = [node.module] if node.module else [a.name for a in node.names]
            elif isinstance(node, ast.ImportFrom) and (node.module or '').startswith('devicehunt'):
                modules = [node.module.split('.', 1)[1]] if '.' in node.module else [a.name for a in node.names]
            elif isinstance(node, ast.Import):
                modules = [a.name.split('.', 1)[1] for a in node.names if a.name.startswith('devicehunt.')]
            for module in modules:
                path = 'devicehunt/' + module.split('.')[0] + '.py'
                if (base / path).is_file():
                    todo.append(path)
    return sorted(seen)


def module_hashes(names):
    """Installed code identity for the full local import closure of `names`."""
    from .artifacts import file_digest
    base = Path(__file__).resolve().parents[1]
    return {name: file_digest(base / name) for name in module_closure(names)}
