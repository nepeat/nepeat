import argparse
import csv
import html
import json
import os
import sqlite3
import sys
import urllib.request
from pathlib import Path
from urllib.parse import quote_plus

from .catalog import Catalog, TARGETS
from . import importers
from . import onie
from . import fcc
from . import edgecore
from . import validation
from . import ntra
from . import ntra_register
from . import ul
from . import tec, marketplace
from . import oracle_evidence
from .pipeline import SOURCE_NAMES, import_source
from .rebuild import rebuild
from . import grantees
from . import icasa
from . import anatel
from . import buying
from . import specifications
from . import bahrain
from . import lookup
from . import ieee
from . import pci
from . import pci_report
from . import documents
from . import listings
from . import vendor_inventory
from . import fcc_coverage
from . import hyperscaler_inventory
from . import tested_components
from . import model_collisions
from . import brand_families

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DB = ROOT / "data/devices.sqlite"


def fetch(url, path, refresh=False, max_bytes=350_000_000):
    path = Path(path)
    if path.exists() and not refresh:
        return path
    path.parent.mkdir(parents=True,exist_ok=True)
    tmp = path.with_suffix(path.suffix+".part")
    request = urllib.request.Request(url,headers={"User-Agent":"devicehunt/0.1 (local public hardware research)"})
    total = 0
    try:
        with urllib.request.urlopen(request,timeout=60) as response, tmp.open("wb") as out:
            while True:
                chunk = response.read(1024*1024)
                if not chunk:
                    break
                total += len(chunk)
                if total > max_bytes:
                    raise ValueError("Download exceeded size limit: " + url)
                out.write(chunk)
        os.replace(tmp,path)
    finally:
        if tmp.exists():
            tmp.unlink()
    return path


def write_rows(rows, fmt, out=None):
    if out is None:
        out = sys.stdout
    if fmt=="json":
        print(json.dumps(rows,ensure_ascii=False,indent=2),file=out)
    elif fmt=="csv":
        if rows:
            writer = csv.DictWriter(out,fieldnames=list(rows[0]))
            writer.writeheader()
            writer.writerows(rows)
    else:
        if not rows:
            print("No matching records.",file=out)
            return
        keys = list(rows[0])
        print("\t".join(keys),file=out)
        for row in rows:
            print("\t".join(str(row[k] if row[k] is not None else "").replace("\n"," ") for k in keys),file=out)


def write_specification_groups(groups, fmt, out=None):
    """Keep nested evidence structured in CSV and expose every member in tables."""
    if fmt == 'json' or not groups:
        return write_rows(groups, fmt, out)
    if fmt == 'csv':
        rows = [{key: value for key, value in group.items() if key != 'evidence'} |
                {'evidence_json': json.dumps(group['evidence'], ensure_ascii=False)} for group in groups]
        return write_rows(rows, fmt, out)
    if out is None:
        out = sys.stdout
    for group in groups:
        shared = {key: value for key, value in group.items() if key != 'evidence'}
        # The strict key makes the raw fact common to every member. Preserve it
        # once when formatting changed the value (for example, DMI citations).
        raw_fact = group['evidence'][0]['raw_fact']
        if raw_fact['value'] != group['statement']:
            shared['raw_fact'] = raw_fact
        print(json.dumps(shared, ensure_ascii=False), file=out)
        for member in group['evidence']:
            provenance = {key: member[key] for key in ('record_id', 'source_id', 'source_key', 'source_url',
                                                      'tier', 'origin', 'source_fact_name', 'affiliations')}
            if 'matched_part_numbers' in member:
                provenance['matched_part_numbers'] = member['matched_part_numbers']
            print('  evidence ' + json.dumps(provenance, ensure_ascii=False), file=out)


def shopping_links(detail):
    # Query links only: not live listings, offers, prices, or availability claims.
    if listings.is_software_reference(detail.get("brand"), detail.get("kind")):
        return []
    candidates = [detail.get("title"), detail.get("model")]
    candidates.extend(i["value"] for i in detail["identifiers"] if i["kind"] in
                      ("fcc_id","part_number","manufacturer_model","model_search","alias","onie_platform"))
    # Keep each nonblank literal; a model query adds no alias or brand inference.
    queries = {q for q in candidates if isinstance(q, str) and q.strip()}
    return [{"query":q,"ebay":"https://www.ebay.com/sch/i.html?_nkw="+quote_plus(q),
             "web":"https://www.google.com/search?q="+quote_plus('"'+q+'" used surplus')}
            for q in sorted(queries)]


def report(c,path):
    rows = c.search(limit=-1)
    source_metadata = {r['id']: r for r in c.db.execute('SELECT id,tier,sha256,cache_path FROM sources')}
    data = []
    for row in rows:
        d = c.detail(row["id"])
        records = {r['id']: r for r in d.pop("records")}
        for a in d['affiliations']:
            source = source_metadata[records[a['record_id']]['source_id']]
            a['source_tier'] = source['tier']
            a['source_snapshot_eligible'] = vendor_inventory.snapshot_eligible(source)
            a['evidence_class'] = vendor_inventory.evidence_class(a['role'])
        d["shopping"] = shopping_links(d)
        data.append(d)
    template = (ROOT/"devicehunt/report.html").read_text()
    payload = json.dumps(data,ensure_ascii=False).replace("<","\\u003c").replace(">","\\u003e").replace("&","\\u0026")
    sources = [dict(r) for r in c.db.execute("SELECT id,tier,records_seen,coverage,fetched_at FROM sources ORDER BY id")]
    source_html = "".join("<tr>"+"".join("<td>"+html.escape(str(s[k] or ""))+"</td>" for k in
                                     ("id","tier","records_seen","coverage"))+"</tr>" for s in sources)
    target_options = ''.join('<option>' + html.escape(target) + '</option>' for target in TARGETS)
    target_names = ' · '.join(html.escape(target) for target in TARGETS)
    Path(path).write_text(template.replace("__DEVICE_DATA__",payload).replace("__SOURCE_ROWS__",source_html)
                         .replace("__TARGET_OPTIONS__",target_options).replace("__TARGET_NAMES__",target_names))


def main(argv=None):
    parser = argparse.ArgumentParser(description="Local SQL and text search for surplus hardware, with source evidence.")
    parser.add_argument("--db",type=Path,default=DEFAULT_DB)
    sub = parser.add_subparsers(dest="command",required=True)
    sub.add_parser("init")
    p = sub.add_parser('evidence', help='Resolve reviewed claims to saved artifacts and lineage offline')
    p.add_argument('query', help='Claim ID, statement ID or exact source key, e.g. Golduck')
    p.add_argument('--data-root', type=Path, default=ROOT / 'data')
    p = sub.add_parser('tested-components', help='Search component rows in sourced tested configurations')
    p.add_argument('query', nargs='?', default='')
    p.add_argument('--vendor', help='Target association on the parent source record')
    p.add_argument('--limit', type=int, default=100, help='-1 for all; 0 for counts only')
    p.add_argument('--format', choices=['table','json','csv'], default='table')
    p = sub.add_parser('fcc-coverage', help='Show local collection gaps for known target-associated FCC IDs')
    p.add_argument('--vendor')
    p.add_argument('--missing', choices=list(fcc_coverage.TABLES))
    p.add_argument('--limit', type=int, default=100, help='-1 for all; 0 for counts only')
    p.add_argument('--format', choices=['table','json','csv'], default='table')
    p = sub.add_parser("rebuild",help="Rebuild all manifest sources offline into a new database; never overwrite")
    p.add_argument("--manifest",type=Path,default=ROOT/"data/import-manifest.json")
    p = sub.add_parser("sync",help="Download/cache and import public source snapshots")
    p.add_argument("--devicecode",action="store_true",help="Also download/import the full pinned ~171 MiB DeviceCode snapshot")
    p.add_argument("--onie",action="store_true",help="Fetch and import all target-vendor ONIE machine definitions at the pinned commit")
    p.add_argument("--edgecore",action="store_true",help="Cache and import English hardware product pages from Edgecore sitemaps")
    p.add_argument("--edgecore-eol",action="store_true",help="Cache and import every notice linked by the Edgecore retirement archive")
    p.add_argument("--fcc-grantees",action="store_true",help="Download the full historical FCC company register, separate from devices")
    p.add_argument("--bahrain",action="store_true",help="Collect Bahrain's public equipment register with consistency checks")
    p.add_argument("--anatel",action="store_true",help="Download Anatel's full published product register twice for consistency checks")
    p.add_argument("--ieee",action="store_true",help="Download the four public IEEE MAC assignment CSV files with consistency checks")
    p.add_argument("--refresh",action="store_true",help="Re-fetch cached sources")
    p = sub.add_parser("import",help="Import cached sources without network")
    p.add_argument("source",choices=SOURCE_NAMES)
    p.add_argument("path",type=Path)
    p = sub.add_parser("grantees",help="Search historical FCC company names and grantee codes locally")
    p.add_argument("query",nargs="?",default="")
    p.add_argument("--limit",type=int,default=100)
    p.add_argument("--format",choices=["table","json","csv"],default="table")
    p = sub.add_parser("registry",help="Search equipment-register rows without inferred vendors")
    p.add_argument("query",nargs="?",default="")
    p.add_argument("--limit",type=int,default=100)
    p.add_argument("--format",choices=["table","json","csv"],default="table")
    p = sub.add_parser("ntra",help="Search all cached Egypt NTRA equipment rows, including non-target brands")
    p.add_argument("query",nargs="?",default="")
    p.add_argument("--limit",type=int,default=100)
    p.add_argument("--format",choices=["table","json","csv"],default="table")
    p = sub.add_parser("anatel",help="Search the complete cached Anatel register by model, company or approval number")
    p.add_argument("query",nargs="?",default="")
    p.add_argument("--limit",type=int,default=100)
    p.add_argument("--format",choices=["table","json","csv"],default="table")
    p = sub.add_parser("search")
    p.add_argument("query",nargs="?",default="")
    p.add_argument("--vendor",help="One of the target companies; matches any recorded role")
    p.add_argument("--role",help="Restrict affiliation, e.g. manufacturer, brand, fcc_grantee, platform_vendor")
    p.add_argument("--hyperscaler",action="store_true",help="Only explicit hyperscaler relationship evidence (not necessarily deployed hardware)")
    p.add_argument("--limit",type=int,default=100)
    p.add_argument("--format",choices=["table","json","csv"],default="table")
    p = sub.add_parser('vendor-inventory', help='List target devices by manufacturer claims, factory listings or all associations')
    p.add_argument('vendor', nargs='?', help='One target vendor; omit for all targets')
    p.add_argument('--scope', choices=['manufacturer','factory','all'], default='manufacturer')
    p.add_argument('--limit', type=int, default=100, help='Target/identity pairs; -1 for all, 0 for counts only')
    p.add_argument('--format', choices=['table','json','csv'], default='table')
    p = sub.add_parser('hyperscaler-inventory', help='Show sourced deployment, lineage and other hyperscaler relationships separately')
    p.add_argument('--vendor', help='Require the target company on the relationship source record')
    p.add_argument('--role', help='Require this exact company role; combined vendor/role filters use one affiliation')
    p.add_argument('--operator', help='Complete operator label, case-insensitive; no inferred aliases')
    p.add_argument('--relationship', help='Exact relationship, e.g. documented_platform_deployment')
    p.add_argument('--include-unverified', action='store_true', help='Also expose seller or uncached claims with their original evidence status')
    p.add_argument('--limit', type=int, default=100, help='Relationship assertions; -1 for all, 0 for counts')
    p.add_argument('--format', choices=['table','json','csv'], default='table')
    p = sub.add_parser('model-collisions', help='Review lead list: identities whose model strings collide under conservative normalisation; never merges')
    p.add_argument('--target', help='Require a member tied to this target company')
    p.add_argument('--limit', type=int, default=20, help='Groups; -1 for all, 0 for counts only')
    p.add_argument('--all-kinds', action='store_true', help='Include software-reference kinds (software/firmware/BMC/device tree/config)')
    p.add_argument('--min-key-length', type=int, default=0, help='Drop groups whose normalised key is shorter than N (default 0: keep all; short keys are flagged)')
    p.add_argument('--format', choices=['table', 'json'], default='table')
    p = sub.add_parser("lookup",help="Search the catalog and all cached raw registries together, without merging evidence")
    p.add_argument("query")
    p.add_argument("--limit",type=int,default=20,help="Rows per dataset; -1 for all, 0 for counts only")
    p.add_argument("--format",choices=["table","json"],default="table")
    p = sub.add_parser("documents",help="Search embedded text in cached vendor, regulator and FCC PDFs")
    p.add_argument("query",nargs="?",default="")
    p.add_argument("--fcc-id")
    p.add_argument("--source",help="Exact document archive source ID, e.g. reference-pdf-aws-panorama")
    p.add_argument("--limit",type=int,default=20)
    p.add_argument("--format",choices=["table","json"],default="table")
    p = sub.add_parser("mac",help="Look up a 48-bit MAC address in the local IEEE snapshot; registered organization only")
    p.add_argument("address")
    p.add_argument("--format",choices=["table","json"],default="table")
    p = sub.add_parser("pci",help="Identify PCI vendor/device and optional subsystem/class IDs in the local community snapshot")
    p.add_argument("identifier",help="hhhh:hhhh vendor/device ID pair")
    p.add_argument("--subsystem",help="hhhh:hhhh subsystem vendor/device ID pair")
    p.add_argument("--class-code",help="Four or six hex digits: class, subclass, optional programming interface")
    p.add_argument("--format",choices=["table","json"],default="table")
    p = sub.add_parser("pci-report",help="Decode a saved numeric lspci -D -vmm -n inventory locally")
    p.add_argument("path",type=Path)
    p.add_argument("--format",choices=["table","json","csv"],default="table")
    p = sub.add_parser("buying-report", help="Report exact record-local buying evidence and collected gaps")
    p.add_argument("query")
    p.add_argument("--vendor")
    p.add_argument("--format", choices=["table", "json"], default="table")
    p = sub.add_parser("specs",help="Search sourced power, cooling, physical and installation statements")
    p.add_argument("query",nargs="?",default="")
    p.add_argument("--vendor",help="Require this company's association on each specification's source record")
    scope = p.add_mutually_exclusive_group()
    scope.add_argument("--exact-model",action="store_true",help="Match complete model strings or aliases; an alias selects only its own source records")
    scope.add_argument("--exact-part-number",action="store_true",help="Show specifications from source records that explicitly identify this complete part number")
    p.add_argument("--include-source-facts",action="store_true",help="Also show reviewed practical source facts (PSU, airflow, U height, weight, form factor, nodes), labelled origin=source fact")
    p.add_argument("--group-identical",action="store_true",help="Group strictly identical statements within each device; keep every evidence member. Limits count complete groups; grouping does not merge configurations or certify agreement")
    p.add_argument("--limit",type=int,default=100)
    p.add_argument("--format",choices=["table","json","csv"],default="table")
    p = sub.add_parser("show")
    p.add_argument("id")
    p = sub.add_parser("shopping")
    p.add_argument("id")
    p = sub.add_parser("sql",help="Read-only SQL; writes are denied")
    p.add_argument("query")
    p.add_argument("--format",choices=["table","json","csv"],default="table")
    sub.add_parser("stats")
    p = sub.add_parser("export")
    p.add_argument("path",type=Path)
    p.add_argument("--format",choices=["json","csv"],default="csv")
    p = sub.add_parser("report",help="Write an offline searchable HTML catalog")
    p.add_argument("path",type=Path)
    p = sub.add_parser("validate-inventory",help="Check owned hardware against imported external snapshots; exit 2 for coverage gaps")
    p.add_argument("--inventory",type=Path,default=ROOT/"validation/owned-hardware.json")
    p.add_argument("--output",type=Path,help="Save the complete evidence/gap report as JSON")
    p.add_argument("--format",choices=["table","json"],default="table")
    p = sub.add_parser('match-listing', help='Match supplied auction titles to catalog names locally; never import seller evidence')
    inputs = p.add_mutually_exclusive_group(required=True)
    inputs.add_argument('--title', help='One listing title')
    inputs.add_argument('--csv', type=Path, help='Local UTF-8 CSV with a title column')
    p.add_argument('--title-column', default='title')
    p.add_argument('--vendor', help='Require a target-company affiliation on each matching evidence record')
    p.add_argument('--include-short-models', action='store_true',help='Also match three-character identifiers containing both letters and digits, e.g. AF1')
    p.add_argument('--include-numeric-identifiers', action='store_true', help='Also match catalog identifiers of 6 to 20 ASCII digits, optionally separated by hyphens; preserves leading zeros')
    p.add_argument('--include-software-references', action='store_true', help='Also match software-reference identities (OpenBMC, ONL, SONiC, ONIE, coreboot, ...) as software_reference_mention, sorted after hardware')
    p.add_argument('--with-specs', action='store_true', help='Include power, cooling, physical and installation statements from each exact matching source record')
    p.add_argument('--limit', type=int, default=20, help='Maximum candidates per title; -1 for all, 0 for counts')
    p.add_argument('--format', choices=['table', 'json', 'csv'], default='table')
    args = parser.parse_args(argv)
    c = None
    try:
        if args.command == 'evidence':
            from .lineage import show
            with sqlite3.connect(args.db.resolve().as_uri() + '?mode=ro', uri=True) as db:
                db.row_factory = sqlite3.Row
                result = show(db, args.query, args.data_root)
            print(json.dumps(result, ensure_ascii=False, indent=2))
            return 0
        if args.command == 'buying-report':
            with sqlite3.connect(args.db.resolve().as_uri() + '?mode=ro', uri=True) as db:
                db.row_factory = sqlite3.Row
                result = buying.run(db, args.query, args.vendor)
            print(json.dumps(result, ensure_ascii=False, indent=2) if args.format == 'json' else buying.render_table(result))
            return 0
        if args.command == 'vendor-inventory':
            with sqlite3.connect(args.db.resolve().as_uri() + '?mode=ro', uri=True) as db:
                db.row_factory = sqlite3.Row
                result = vendor_inventory.run(db, args.vendor, args.scope, args.limit)
            if args.format == 'json':
                print(json.dumps(result, ensure_ascii=False, indent=2))
            elif args.format == 'csv':
                write_rows(vendor_inventory.flat_rows(result), 'csv')
            else:
                print(result['scope_description'])
                write_rows(result['counts_by_target'], 'table')
                print(f"Showing {result['shown']} of {result['identity_target_pairs']} target/identity pairs; {result['unique_identities']} distinct identities.")
                if result['truncated']: print('Use --limit -1 for the complete collected inventory.')
                write_rows([{**{k:r[k] for k in ('target','device_id','brand','model')},
                             'roles':', '.join(sorted({e['role'] for e in r['evidence']})),
                             'sources':len({e['source_id'] for e in r['evidence']})} for r in result['rows']], 'table')
            return 0
        if args.command == 'match-listing':
            inputs = listings.read_csv(args.csv, args.title_column) if args.csv else [
                {'input_record': 1, 'input': {'title': args.title}, 'title': args.title}]
            with sqlite3.connect(args.db.resolve().as_uri() + '?mode=ro', uri=True) as db:
                db.row_factory = sqlite3.Row
                matcher = listings.Matcher(db, args.vendor, include_short_models=args.include_short_models,
                                           include_numeric_identifiers=args.include_numeric_identifiers,
                                           include_specifications=args.with_specs,
                                           include_software_references=args.include_software_references,
                                           families=brand_families.load_default())
                results = [dict(item, result=matcher.match(item['title'], args.limit)) for item in inputs]
            if args.format == 'json':
                print(json.dumps({'scope': listings.SCOPE, 'listing_count': len(results), 'results': results}, ensure_ascii=False, indent=2))
            else:
                if args.format == 'table':
                    print(listings.SCOPE)
                    if results and 'brand_families' in results[0]['result']:
                        bf = results[0]['result']['brand_families']
                        print(f"brand families (display only): {bf['path']} sha256={bf['sha256'][:12]}; {bf['rows_pending_review']} of {bf['rows']} rows pending review")
                    if args.with_specs:
                        print(listings.SPECIFICATION_SCOPE)
                write_rows([row for item in results for row in listings.flat_rows(item['result'], item['input_record'])], args.format)
            return 0
        if args.command=="rebuild":
            result = rebuild(args.manifest,args.db,lambda row: print(json.dumps(row),flush=True))
            print(json.dumps(result))
            return 0
        if args.command=="documents":
            with sqlite3.connect(args.db.resolve().as_uri()+"?mode=ro",uri=True) as db:
                db.row_factory=sqlite3.Row
                result=documents.search(db,args.query,args.fcc_id,args.limit,source=args.source)
            if args.format=="json":
                print(json.dumps(result,ensure_ascii=False,indent=2))
            else:
                print(f"{result['matches']} matching PDF pages ({result['state']})")
                print(result.get('scope','No document archive loaded.'))
                if result.get('truncated'): print('More matches available with --limit -1.')
                write_rows([{k:r[k] for k in ('source_id','fcc_id','document_id','title','page_number','text_status','excerpt','local_pdf')} for r in result['rows']],'table')
            return 0
        if args.command=="mac":
            with sqlite3.connect(args.db.resolve().as_uri()+"?mode=ro",uri=True) as db:
                db.row_factory=sqlite3.Row
                result=ieee.mac_lookup(db,args.address)
            if args.format=="json":
                print(json.dumps(result,ensure_ascii=False,indent=2))
            else:
                print(f"{result['mac']}: {result['classification']}; {result['status']} ({result['state']})")
                print(result.get('reason',result['scope']))
                if result['matches']:
                    write_rows(result['matches'],'table')
            return 0
        if args.command=="pci-report":
            with args.path.open('rb') as stream:
                body=stream.read(pci_report.MAX_BYTES+1)
            with sqlite3.connect(args.db.resolve().as_uri()+"?mode=ro",uri=True) as db:
                db.row_factory=sqlite3.Row
                result=pci_report.analyze(db,body)
            if args.format=="json":
                print(json.dumps(result,ensure_ascii=False,indent=2))
            else:
                if args.format=="table":
                    print(result['scope'])
                    print(json.dumps(result['summary']))
                write_rows(pci_report.table_rows(result),args.format)
            return 0
        if args.command=="pci":
            with sqlite3.connect(args.db.resolve().as_uri()+"?mode=ro",uri=True) as db:
                db.row_factory=sqlite3.Row
                result=pci.identify(db,args.identifier,args.subsystem,args.class_code)
            if args.format=="json":
                print(json.dumps(result,ensure_ascii=False,indent=2))
            else:
                print(result['id'] + ': ' + result['state'])
                print(result['scope'])
                for kind in ['vendor','device','subsystem_vendor','subsystem','class']:
                    if kind in ('subsystem_vendor','subsystem') and args.subsystem is None:
                        continue
                    if kind=='class' and args.class_code is None:
                        continue
                    rows=result[kind]
                    print(kind + ': ' + ('matched' if rows else 'no match'))
                    if rows:
                        write_rows([{k:r[k] for k in ['entry_kind','id_path','name','line_number']} for r in rows],'table')
            return 0
        if args.command=='tested-components':
            with sqlite3.connect(args.db.resolve().as_uri()+'?mode=ro',uri=True) as db:
                db.row_factory=sqlite3.Row
                result=tested_components.run(db,args.query,args.vendor,args.limit)
            if args.format=='json':
                print(json.dumps(result,ensure_ascii=False,indent=2))
            else:
                if args.format=='table':
                    print(result['scope'])
                    print(str(result['matches'])+' matching component rows; '+str(result['shown'])+' shown.')
                    fields=('parent_model','configuration_id','component_kind','component_manufacturer',
                            'component_model','quantity','source_page','original_document_status')
                    write_rows([{k:r[k] for k in fields} for r in result['rows']],'table')
                else:
                    write_rows(result['rows'],args.format)
            return 0
        if args.command=='hyperscaler-inventory':
            with sqlite3.connect(args.db.resolve().as_uri()+'?mode=ro', uri=True) as db:
                db.row_factory=sqlite3.Row
                result=hyperscaler_inventory.run(db,args.vendor,args.role,args.operator,
                                                args.relationship,args.include_unverified,args.limit)
            if args.format=='json':
                print(json.dumps(result,ensure_ascii=False,indent=2))
            elif args.format=='csv':
                write_rows(hyperscaler_inventory.flat_rows(result),'csv')
            else:
                print(result['scope_description'])
                print(f"{result['assertions']} relationship assertions across {result['unique_identities']} identities; {result['shown']} shown.")
                write_rows([{**{key:row[key] for key in (
                    'brand','model','revision','operator','relationship','detail','source_id',
                    'source_url','source_snapshot_eligible')},
                    'company_roles':'; '.join(a['company']+' ('+a['role']+')' for a in row['affiliations']),
                    'source_dates':'; '.join(d['field']+'='+d['value'] for d in row['source_dates'])}
                    for row in result['rows']],'table')
            return 0
        if args.command=='fcc-coverage':
            with sqlite3.connect(args.db.resolve().as_uri()+'?mode=ro', uri=True) as db:
                db.row_factory=sqlite3.Row
                result=fcc_coverage.run(db,args.vendor,args.missing,args.limit)
            if args.format=='json':
                print(json.dumps(result,ensure_ascii=False,indent=2))
            else:
                if args.format=='table':
                    print(result['scope'])
                    print(json.dumps(result['summary']))
                write_rows(fcc_coverage.flat_rows(result),args.format)
            return 0
        if args.command == 'model-collisions':
            with sqlite3.connect(args.db.resolve().as_uri() + '?mode=ro', uri=True) as db:
                db.row_factory = sqlite3.Row
                result = model_collisions.run(db, args.target, args.limit, args.all_kinds, args.min_key_length,
                                              families=brand_families.load_default())
            print(json.dumps(result, ensure_ascii=False, indent=2) if args.format == 'json' else model_collisions.render_table(result))
            return 0
        if args.command=="lookup":
            with sqlite3.connect(args.db.resolve().as_uri()+"?mode=ro",uri=True) as db:
                db.row_factory=sqlite3.Row
                result=lookup.run(db,args.query,args.limit,families=brand_families.load_default())
            if args.format=="json":
                print(json.dumps(result,ensure_ascii=False,indent=2))
            else:
                print(result['matching'])
                print(result['scope'])
                if 'brand_families' in result:
                    bf=result['brand_families']
                    print(f"brand families (display only): {bf['path']} sha256={bf['sha256'][:12]}; {bf['rows_pending_review']} of {bf['rows']} rows pending review")
                for dataset in result['datasets']:
                    suffix='; more matches available with --limit -1' if dataset['truncated'] else ''
                    print(f"\n{dataset['dataset']}: {dataset['matches']} matches; showing {dataset['shown']} "
                          f"({dataset['unit']}; {dataset['state']}{suffix})")
                    print(dataset['scope'])
                    rows=[]
                    for entry in dataset['rows']:
                        row={k:v for k,v in entry.items() if k not in ('evidence','identity_text_matches','matched_in','matched_in_detail','family_members_hit')}
                        if 'evidence' in entry:
                            row['source_ids']=', '.join(sorted({e['source_id'] for e in entry['evidence']}))
                            row['evidence_tiers']=', '.join(sorted({e['tier'] for e in entry['evidence']}))
                            row['affiliation_roles']=', '.join(sorted({e['role'] for e in entry['evidence'] if e['role']}))
                            row['identity_text_match']=bool(entry['identity_text_matches'])
                            row['matched_in']=entry['matched_in']
                        if 'brand_family' in row: row['brand_family']=row['brand_family'] or ''
                        rows.append(row)
                    if rows:
                        write_rows(rows,'table')
            return 0
        if args.command=="validate-inventory":
            with sqlite3.connect(args.db.resolve().as_uri()+"?mode=ro",uri=True) as db:
                db.row_factory=sqlite3.Row
                result=validation.run(db,args.inventory)
            if args.output:
                args.output.parent.mkdir(parents=True,exist_ok=True)
                args.output.write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n")
            if args.format=="json":
                print(json.dumps(result,ensure_ascii=False,indent=2))
            else:
                write_rows([{"identifier":r["identifier"],"vendor":r["vendor"],"status":r["status"],
                             "search_hits":len(r["search_hits"]),"evidence":len(r["evidence"]),
                             "leads":len(r["leads"])} for r in result["results"]],"table")
                s=result["summary"]
                print(f"\nSearch results: {result['search_summary']['queries_with_results']}/{s['total']} queries.")
                print(f"Confirmed exact identities: {s['hits']}/{s['total']}; {s['gaps']} gaps. AWS associations are owner-provided context.")
            return 2 if result["summary"]["gaps"] else 0
        if args.command=="sql":
            with sqlite3.connect(args.db.resolve().as_uri()+"?mode=ro",uri=True) as db:
                db.row_factory=sqlite3.Row
                # FTS initializes virtual-table metadata lazily. Do that before
                # installing the user-query authorizer, still on a read-only DB.
                for table in ('search_index', 'document_search'):
                    if db.execute("SELECT 1 FROM sqlite_master WHERE name=? AND type='table'", (table,)).fetchone():
                        db.execute('SELECT count(*) FROM ' + table).fetchone()
                allowed = {sqlite3.SQLITE_SELECT,sqlite3.SQLITE_READ,sqlite3.SQLITE_FUNCTION,sqlite3.SQLITE_RECURSIVE}
                def authorize(action, arg1, arg2, database, trigger):
                    if action in allowed or (action==sqlite3.SQLITE_PRAGMA and arg1=="data_version" and arg2 is None):
                        return sqlite3.SQLITE_OK
                    return sqlite3.SQLITE_DENY
                db.set_authorizer(authorize)
                write_rows([dict(r) for r in db.execute(args.query)],args.format)
            return 0
        c = Catalog(args.db, read_only=args.command not in {'init', 'import', 'sync'})
        if args.command=="import":
            result = import_source(c,args.source,args.path)
            print(json.dumps({args.source:result}))
        elif args.command=="sync":
            raw = ROOT/"data/raw"
            jobs = [("sonic",importers.SONIC_URL,raw/"sonic.html"),
                    ("onl",importers.ONL_URL,raw/"onl.md")]
            jobs += [(sid,url,raw/(sid+".html")) for sid,url in importers.ORACLE_URLS.items()]
            jobs += [(sid,url,raw/(sid+".html")) for sid,url in importers.HARDWARE_INDEXES.items()]
            if args.devicecode:
                jobs.append(("devicecode","https://codeload.github.com/armijnhemel/devicecode-data/tar.gz/"+
                             importers.DEVICECODE_REV,raw/"devicecode-138e8afd.tar.gz"))
            for sid,url,path in jobs:
                fetch(url,path,args.refresh)
                if sid in importers.HARDWARE_INDEXES:
                    result=importers.hardware_index(c,path,sid)
                elif sid in importers.ORACLE_URLS:
                    result=importers.oracle(c,path,sid)
                else:
                    result=getattr(importers,sid)(c,path)
                print(json.dumps({sid:result}),flush=True)
            if args.onie:
                bundle=onie.collect(raw,fetch,args.refresh)
                print(json.dumps({"onie":onie.import_bundle(c,bundle)}),flush=True)
            if args.edgecore:
                bundle=edgecore.collect(raw,fetch,args.refresh)
                print(json.dumps({"edgecore":edgecore.import_bundle(c,bundle)}),flush=True)
            if args.edgecore_eol:
                bundle=edgecore.collect_notices(raw,fetch,args.refresh)
                print(json.dumps({"edgecore-notices":edgecore.import_notices(c,bundle)}),flush=True)
            if args.fcc_grantees:
                bundle=grantees.collect(raw,fetch,args.refresh)
                print(json.dumps({"fcc-grantees":grantees.import_bundle(c,bundle)}),flush=True)
            if args.bahrain:
                bundle=bahrain.collect(raw,fetch,args.refresh)
                print(json.dumps({"bahrain":bahrain.import_bundle(c,bundle)}),flush=True)
            if args.anatel:
                bundle=anatel.collect(raw,fetch,args.refresh)
                print(json.dumps({"anatel":anatel.import_bundle(c,bundle)}),flush=True)
            if args.ieee:
                bundle=ieee.collect(raw,fetch,args.refresh)
                print(json.dumps({"ieee":ieee.import_bundle(c,bundle)}),flush=True)
            print(json.dumps({"curated":importers.curated(c,ROOT/"data/curated.json")}))
            print(json.dumps({"fcc":fcc.import_manifest(c,ROOT/"data/fcc-manifest.json")}))
        elif args.command=="anatel":
            write_rows(anatel.search(c.db,args.query,args.limit),args.format)
        elif args.command=="ntra":
            write_rows(ntra_register.search(c.db,args.query,args.limit),args.format)
        elif args.command=="search":
            write_rows(c.search(args.query,args.vendor,args.hyperscaler,args.limit,args.role),args.format)
        elif args.command=="specs":
            rows = specifications.search(c,args.query,args.vendor,args.limit,args.exact_model,args.exact_part_number,args.include_source_facts,group_identical=args.group_identical)
            if args.group_identical:
                write_specification_groups(rows,args.format)
            else:
                write_rows(rows,args.format)
        elif args.command=="grantees":
            write_rows(grantees.search(c.db,args.query,args.limit),args.format)
        elif args.command=="registry":
            write_rows(icasa.search(c.db,args.query,args.limit),args.format)
        elif args.command=="show":
            print(json.dumps(c.detail(args.id),ensure_ascii=False,indent=2))
        elif args.command=="shopping":
            detail = c.detail(args.id)
            if listings.is_software_reference(detail.get("brand"), detail.get("kind")):
                print("Software reference; no hardware shopping links.")
            else:
                write_rows(shopping_links(detail),"table")
        elif args.command=="stats":
            print("Target associations (a device may appear under multiple companies):")
            write_rows([dict(r) for r in c.db.execute("SELECT target,COUNT(DISTINCT id) AS devices FROM target_devices GROUP BY target ORDER BY target")],"table")
            print("\nSource coverage:")
            write_rows([dict(r) for r in c.db.execute("SELECT id,tier,records_seen,coverage FROM sources ORDER BY id")],"table")
            print("\nUnique catalog identities:",c.db.execute("SELECT count(*) FROM devices").fetchone()[0])
            print("Historical FCC company registrations (separate from devices):",c.db.execute("SELECT count(*) FROM fcc_grantees").fetchone()[0])
            print("Equipment-register rows (without inferred vendors):",c.db.execute("SELECT count(*) FROM equipment_register").fetchone()[0])
            print("Bahrain manufacturer-register rows:",c.db.execute("SELECT count(*) FROM bahrain_equipment").fetchone()[0])
            print("Anatel published product-register rows:",c.db.execute("SELECT count(*) FROM anatel_equipment").fetchone()[0])
            print("NTRA published equipment-register rows:",c.db.execute("SELECT count(*) FROM ntra_equipment").fetchone()[0])
            print("TEC historical equipment-register rows:",c.db.execute("SELECT count(*) FROM tec_equipment").fetchone()[0])
            print("IEEE published address-assignment rows:",c.db.execute("SELECT count(*) FROM ieee_assignments").fetchone()[0])
            print("Community PCI name entries:",c.db.execute("SELECT count(*) FROM pci_entries").fetchone()[0])
            print("Taiwan BSMI certificate rows:",c.db.execute("SELECT count(*) FROM bsmi_certificates").fetchone()[0])
            print("Taiwan BSMI product certificate rows:",c.db.execute("SELECT count(*) FROM bsmi_product_certificates").fetchone()[0])
            print("Taiwan BSMI voluntary certificate rows:",c.db.execute("SELECT count(*) FROM bsmi_voluntary_certificates").fetchone()[0])
            print("Taiwan BSMI declaration rows:",c.db.execute("SELECT count(*) FROM bsmi_declarations").fetchone()[0])
            print("Korean RRA approval rows:",c.db.execute("SELECT count(*) FROM rra_equipment").fetchone()[0])
            print("Canadian REL query CSV rows:",c.db.execute("SELECT count(*) FROM ised_equipment").fetchone()[0])
            print("Nigeria NCC equipment rows:",c.db.execute("SELECT count(*) FROM nigeria_equipment").fetchone()[0])
            print("Ghana NCA equipment rows:",c.db.execute("SELECT count(*) FROM ghana_equipment").fetchone()[0])
            print("Additional public register rows:",c.db.execute("SELECT count(*) FROM public_equipment_rows").fetchone()[0])
            print("Inspur support-menu rows:",c.db.execute("SELECT count(*) FROM inspur_support_entries").fetchone()[0])
            print("ENERGY STAR network/storage rows:",c.db.execute("SELECT count(*) FROM energystar_datacenter").fetchone()[0])
            print("Historical ENERGY STAR rows:",c.db.execute("SELECT count(*) FROM energystar_server_archives").fetchone()[0])
            print("ENERGY STAR server rows:",c.db.execute("SELECT count(*) FROM energystar_servers").fetchone()[0])
            print("Taiwan NCC published equipment rows:",c.db.execute("SELECT count(*) FROM ncc_equipment").fetchone()[0])
            print("FCC mirror index entries (selected grantees):",c.db.execute("SELECT count(*) FROM fcc_id_index").fetchone()[0])
            print("FCC detail pages / exhibit links / text excerpts:",
                  *[c.db.execute('SELECT count(*) FROM '+t).fetchone()[0]
                    for t in ('fcc_detail_pages','fcc_detail_links','fcc_text_excerpts')])
            print("Cached PDF documents/pages:",c.db.execute("SELECT count(*) FROM documents").fetchone()[0],
                  c.db.execute("SELECT count(*) FROM document_pages").fetchone()[0])
        elif args.command=="export":
            args.path.parent.mkdir(parents=True,exist_ok=True)
            rows = [dict(r) for r in c.db.execute("SELECT * FROM target_devices ORDER BY target,brand,model")]
            with args.path.open("w",newline="") as out:
                write_rows(rows,args.format,out)
            print(args.path.resolve())
        elif args.command=="report":
            args.path.parent.mkdir(parents=True,exist_ok=True)
            report(c,args.path)
            print(args.path.resolve())
        elif args.command=="init":
            c.db.commit()
            print(args.db.resolve())
        return 0
    except (ValueError,OSError,sqlite3.Error,KeyError) as exc:
        if c:
            c.db.rollback()
        print("error: "+str(exc),file=sys.stderr)
        return 1
    finally:
        if c:
            c.db.close()


if __name__=="__main__":
    raise SystemExit(main())
