"""Eight read-only cases; presentation prototype only, no production changes."""
from collections import Counter
import contextlib
import datetime as dt
import hashlib
import io
import json
from pathlib import Path
import sys
from unittest.mock import patch

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
sys.path.insert(0,str(ROOT))
from devicehunt.catalog import Catalog
from devicehunt import specifications as specs
from devicehunt.__main__ import main as cli


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    db=ROOT/'data/devices.sqlite'
    before=sha(db)
    c=Catalog(db,read_only=True)
    cases=[dict(name='nf5280m7',query='NF5280M7',exact_model=True),
           dict(name='as7326',query='AS7326-56X',exact_model=True),
           dict(name='oracle7420',query='Sun ZFS Storage 7420',exact_model=True),
           dict(name='as6700',query='AS6700-32X',exact_model=True),
           dict(name='nf5280-discovery',query='NF5280',exact_model=False),
           dict(name='ais800-accton',query='AIS800-32O',exact_model=True,target='Accton'),
           dict(name='ds4000-part-number',query='R4023-F9102-A0',exact_part_number=True,target='Celestica'),
           dict(name='global-presentation-duplicate-scope',query='',exact_model=False)]
    results=[]
    for case in cases:
        args=['--db',str(db),'specs',case['query'],'--include-source-facts','--format','json']
        if case.get('exact_model'):args.append('--exact-model')
        if case.get('exact_part_number'):args.append('--exact-part-number')
        if case.get('target'):args.extend(['--vendor',case['target']])
        out=io.StringIO()
        with contextlib.redirect_stdout(out):assert cli(args)==0
        output=out.getvalue()
        (HERE/(case['name']+'-baseline-cli.json')).write_text(output)
        kwargs=dict(query=case['query'],target=case.get('target'),limit=-1,exact_model=case.get('exact_model',False),exact_part_number=case.get('exact_part_number',False),include_source_facts=True)
        baseline=specs.search(c,**kwargs)
        assert json.loads(output)==baseline[:100]
        # In-memory read-only diagnostic bypass of existing display collapse;
        # no importer, temporary database or production source edit is involved.
        with patch.object(specs,'dedupe_source_facts',side_effect=lambda rows:rows):
            raw=specs.search(c,**kwargs)
        enriched=[]
        for row in raw:
            fact_name=row.get('source_fact_name') or row['category']
            facts=[dict(f) for f in c.db.execute('SELECT name,value FROM facts WHERE record_id=? AND name=? ORDER BY value',(row['record_id'],fact_name))]
            matches=[f for f in facts if (specs.source_fact_statement(f['name'],f['value']) if row.get('origin')=='source fact' else f['value'])==row['statement']]
            assert len(matches)==1
            affiliations=[dict(a) for a in c.db.execute('SELECT company,target,role FROM affiliations WHERE record_id=? ORDER BY company,role',(row['record_id'],))]
            record=dict(c.db.execute('SELECT source_key,source_url FROM records WHERE id=?',(row['record_id'],)).fetchone())
            enriched.append(dict(row,raw_fact=matches[0],affiliations=affiliations,source_key=record['source_key']))
        groups={}
        for row in enriched:
            key=(row['device_id'],row['category'],row.get('origin','curated statement'),row.get('source_fact_name',''),row['statement'],row['raw_fact']['value'])
            if key not in groups:
                groups[key]={k:row[k] for k in ('device_id','brand','model','revision','category','statement')}
                groups[key].update(origin=row.get('origin','curated statement'),source_fact_name=row.get('source_fact_name',''),evidence=[])
            groups[key]['evidence'].append(row)
        grouped=list(groups.values())
        for group in grouped:
            group.update(evidence_count=len(group['evidence']),record_count=len({r['record_id'] for r in group['evidence']}),source_count=len({r['source_id'] for r in group['evidence']}))
        same_source=Counter((r['device_id'],r['category'],r['statement'],r.get('source_fact_name'),r['source_id']) for r in raw)
        mixed_brands={}
        for row in raw:mixed_brands.setdefault(row['model'],set()).add(row['brand'])
        displays=Counter((r['device_id'],r['category'],r['statement']) for r in baseline)
        overlap=[]
        for key in displays:
            origins={r.get('origin') for r in baseline if (r['device_id'],r['category'],r['statement'])==key}
            if len(origins)>1:overlap.append(dict(device_id=key[0],category=key[1],statement=key[2],origins=sorted(origins)))
        artifact=dict(case=case,actual_baseline_cli_argv=args,baseline_full_rows=baseline,precollapse_full_rows=enriched,prototype_groups=grouped,
                      scope='read-only diagnostic/prototype; grouped output is not implemented or accepted',baseline_cli_sha256=sha(HERE/(case['name']+'-baseline-cli.json')))
        (HERE/(case['name']+'-probe.json')).write_text(json.dumps(artifact,indent=2,ensure_ascii=False)+'\n')
        result=dict(case=case,baseline_full_rows=len(baseline),baseline_default_100_rows=min(len(baseline),100),precollapse_rows=len(raw),strict_literal_groups=len(grouped),
                    strict_group_default_100_rows=min(len(grouped),100),baseline_first_100_distinct_display_statements=len({(r['device_id'],r['category'],r['statement']) for r in baseline[:100]}),
                    first_5_distinct_display_statements=len({(r['device_id'],r['category'],r['statement']) for r in baseline[:5]}),grouped_first_5_distinct_display_statements=len({(g['device_id'],g['category'],g['statement']) for g in grouped[:5]}),
                    repeated_same_source_keys=sum(v>1 for v in same_source.values()),max_repeat=max((g['evidence_count'] for g in grouped),default=0),
                    exact_models_with_multiple_brands={m:sorted(b) for m,b in mixed_brands.items() if len(b)>1},display_origin_overlap_groups=len(overlap),
                    baseline_cli_sha256=artifact['baseline_cli_sha256'],probe_sha256=sha(HERE/(case['name']+'-probe.json')))
        results.append(result)
    c.db.close()
    after=sha(db)
    assert before==after
    report=dict(at_utc=dt.datetime.now(dt.timezone.utc).isoformat(),database='data/devices.sqlite',sha256_before=before,sha256_after=after,unchanged=True,
                code_hashes={n:sha(ROOT/n) for n in ('devicehunt/specifications.py','devicehunt/__main__.py','devicehunt/catalog.py','tests/test_specifications.py','tests/test_specifications_source_facts.py','docs/specification-search.md')},
                cases=results,total_cases=8,current_CLI_default_limit=100,prototype_group_key=['device_id','category','origin','source_fact_name','displayed statement','raw fact value'],
                imports=0,network=0,production_edits=0,scope='eight read-only representative queries, actual current CLI/API output plus in-memory presentation prototype; no current behavior change')
    (HERE/'probe-results.json').write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k!='code_hashes'},indent=2))


if __name__=='__main__':main()
