"""Import snapshots, keeping source claims separate and auditable."""
import json
import re
import tarfile
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import quote, urljoin

from .catalog import Catalog, target_for

DEVICECODE_REV = "138e8afdef93488a430cbf9d62cd0ef84e4e1e7f"
SONIC_URL = "https://sonic-net.github.io/SONiC/Supported-Devices-and-Platforms.html"
ONL_URL = "https://raw.githubusercontent.com/opencomputeproject/OpenNetworkLinux/master/docs/SupportedHardware.md"
ORACLE_URLS = {
    "oracle-x86": "https://docs.oracle.com/en/servers/",
    "oracle-previous": "https://docs.oracle.com/en/servers/previousreleases.html",
    "oracle-sparc": "https://docs.oracle.com/en/servers/sparc.html",
    "oracle-netra": "https://docs.oracle.com/en/servers/netra.html",
}

# These indexes contain hardware outside the initial server-only scope.
HARDWARE_INDEXES = {
    "celestica-storage": "https://www.celestica.com/our-expertise/platform-solutions/storage",
    "oracle-storage": "https://docs.oracle.com/en/storage/",
    "oracle-ethernet": "https://docs.oracle.com/en/networking/ethernet.html",
    "oracle-unified-storage": "https://docs.oracle.com/cd/F24631_01/index.html",
    "oracle-eol": "https://www.oracle.com/servers/eol/",
}


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__(convert_charrefs=True)
        self.rows, self.links = [], []
        self.row = self.cell = self.anchor = None
        self.href = ""
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        if tag == "tr":
            self.row = []
        elif tag in ("td", "th"):
            self.cell = []
        elif tag == "a":
            self.anchor = []
            self.href = dict(attrs).get("href", "")
        elif tag == "br" and self.cell is not None:
            self.cell.append(" ")

    def handle_data(self, data):
        if self.cell is not None:
            self.cell.append(data)
        if self.anchor is not None:
            self.anchor.append(data)

    def handle_endtag(self, tag):
        if tag in ("td", "th") and self.cell is not None:
            if self.row is not None:
                self.row.append(" ".join("".join(self.cell).split()))
            self.cell = None
        elif tag == "tr" and self.row is not None:
            self.rows.append(self.row)
            self.row = None
        elif tag == "a" and self.anchor is not None:
            self.links.append((" ".join("".join(self.anchor).split()),self.href))
            self.anchor = None


class LinkedRows(Page):
    """Keep table links associated with the exact cell that contains them."""
    def __init__(self, text):
        self.linked_rows, self.cell_links = [], []
        super().__init__(text)

    def handle_starttag(self, tag, attrs):
        super().handle_starttag(tag, attrs)
        if tag == 'tr':
            self.cell_links = []

    def handle_endtag(self, tag):
        if tag == 'a' and self.anchor is not None and self.row is not None and self.cell is not None:
            self.cell_links.append((len(self.row), ' '.join(''.join(self.anchor).split()), self.href))
        if tag == 'tr' and self.row is not None:
            self.linked_rows.append((self.row, self.cell_links))
        super().handle_endtag(tag)


def devicecode(c, path, revision=DEVICECODE_REV):
    sid = "devicecode"
    c.source(sid,"https://github.com/armijnhemel/devicecode-data", "community",
             "Mixed: CC-BY-SA-3.0/4.0; FCC metadata CC0; OUI overlays GPL-2.0; see upstream LICENSE.md",
             "Full squashed/devices snapshot scanned for exact target brand, manufacturer, or FCC grantee. Not all worldwide devices.",path,revision)
    seen = matched = 0
    # Stream archive members without extracting paths supplied by upstream.
    with tarfile.open(path, "r|gz") as archive:
        for entry in archive:
            if not entry.isfile() or "/squashed/devices/" not in entry.name or not entry.name.endswith(".json"):
                continue
            if entry.size > 5_000_000:
                raise ValueError("Unexpectedly large DeviceCode record: " + entry.name)
            d = json.load(archive.extractfile(entry))
            seen += 1
            brand = d.get("brand", "")
            maker = d.get("manufacturer", {})
            fcc = d.get("regulatory", {}).get("fcc_ids", [])
            affiliations = [(brand,"brand"),(maker.get("name", ""),"manufacturer")]
            affiliations += [(f.get("grantee", ""), "fcc_grantee") for f in fcc]
            if not any(target_for(name) for name, _ in affiliations):
                continue
            title = d.get("title") or Path(entry.name).stem
            model = d.get("model", {})
            ids = [("fcc_id", f.get("fcc_id")) for f in fcc]
            ids += [("ic_id", i) for i in d.get("regulatory",{}).get("industry_canada_ids",[])]
            for key in ("part_number","pcb_id"):
                ids.append((key,model.get(key,"")))
            if maker.get("model"):
                ids.append(("manufacturer_model",maker["model"]))
            facts = []
            for part in ("cpus","switch","ram","flash","additional_chips"):
                for chip in d.get(part,[]):
                    facts.append((part,json.dumps(chip,ensure_ascii=False)))
            for key in ("network","software","power","power_supply","commercial","serial","origins"):
                if d.get(key):
                    facts.append((key,json.dumps(d[key],ensure_ascii=False)))
            relpath = "squashed/devices/" + Path(entry.name).name
            c.add(sid,relpath,"https://github.com/armijnhemel/devicecode-data/blob/"+revision+"/"+quote(relpath),
                  brand, model.get("model") or title,title,", ".join(d.get("device_types",[])),d,
                  affiliations,ids,facts,revision=" ".join(str(model.get(k,"")) for k in ("revision","submodel","subrevision")).strip())
            matched += 1
    if seen == 0:
        raise ValueError("No consolidated DeviceCode records found; check archive layout")
    c.finish(sid,seen)
    return {"scanned":seen,"matched":matched}


def sonic(c,path):
    c.source("sonic",SONIC_URL,"project","See SONiC upstream terms; factual platform metadata",
             "Target-vendor rows in SONiC supported platforms table. Listing is not proof of current image availability or deployment.",path)
    page = Page(Path(path).read_text())
    count = 0
    for row in page.rows:
        if len(row)<5 or not target_for(row[0]):
            continue
        vendor,model,asic_vendor,asic,ports = row[:5]
        c.add("sonic",vendor+":"+model,SONIC_URL,vendor,model,vendor+" "+model,"switch",row,
              [(vendor,"platform_vendor")],facts=[("asic_vendor",asic_vendor),("asic",asic),("ports",ports),
              ("software_evidence","Listed in SONiC platform table; check release and exact hardware revision before purchase")])
        count += 1
    if not count:
        raise ValueError("SONiC parser found no target rows; source layout may have changed")
    c.finish("sonic",count)
    return count


def onl(c,path):
    c.source("onl",ONL_URL,"project","See OpenNetworkLinux repository license; factual platform metadata",
             "Historical ONL hardware table, dated 2018-05-03. Reviewed Accton/Edge-Core, Celestica, Inventec and Quanta sections. Section headings are platform-vendor associations, not per-model OEM proof or current support guarantees. Original CPU/forwarding cells remain verbatim, including incomplete cells.",path)
    section = ""
    count = 0
    for line in Path(path).read_text().splitlines():
        if line.startswith("## "):
            section = line[3:].strip()
        elif line.strip() and "|" not in line and not re.fullmatch(r"[-=]+",line.strip()):
            section = line.strip()
        if section not in ("Accton/Edge-Core","Celestica","Inventec","Quanta") or "|" not in line:
            continue
        row = [x.strip() for x in line.strip().strip("|").split("|")]
        if len(row)<4 or row[0].lower()=="device" or re.fullmatch(r"[- :]+",row[0]):
            continue
        model,ports,cpu,asic = row[:4]
        vendor = "Accton" if section=="Accton/Edge-Core" else section
        affiliations = [(vendor,"platform_vendor")]
        if section=="Accton/Edge-Core":
            affiliations.append(("Edge-Core","platform_vendor"))
        c.add("onl",section+":"+model,ONL_URL,vendor,model,vendor+" "+model,"switch",row,
              affiliations,facts=[("ports",ports),("cpu",cpu),("asic",asic),
              ("software_evidence","Historical ONL listing from 2018; not a current support guarantee")])
        count += 1
    if not count:
        raise ValueError("ONL parser found no target rows")
    c.finish("onl",count)
    return count


def oracle(c,path,sid):
    url = ORACLE_URLS[sid]
    c.source(sid,url,"vendor","Oracle documentation; extracted factual product names and links",
             "Product links on one Oracle server documentation index; includes legacy Sun products, excludes Fujitsu-branded models. Not OEM manufacturing evidence.",path)
    count = 0
    for title,href in Page(Path(path).read_text()).links:
        if not re.match(r"^(Oracle Server |Sun Server |Sun Blade |SPARC [STM]\d|Netra (?:SPARC |Blade |Server |Modular )|Oracle Netra |Sun Netra )",title):
            continue
        # Index can group products under one documentation link. Keep group explicit.
        model = re.sub(r"^(Oracle Server |Sun Server |Sun Blade |Oracle Netra |Sun Netra |Netra )", "", title)
        c.add(sid,title,urljoin(url,href),"Oracle",title,title,"server",{"title":title,"href":href},
              [("Oracle","documentation_vendor")],[("model_search",model)],
              [("family",sid.removeprefix("oracle-")),("index_url",url),
               ("scope_note","Documentation catalog entry; a grouped title may cover multiple models")])
        count += 1
    if not count:
        raise ValueError("Oracle parser found no model links: " + sid)
    c.finish(sid,count)
    return count


def _curated_required(value, fields, label):
    if not isinstance(value, dict):
        raise ValueError('Invalid curated input: ' + label + ' must be an object')
    for field in fields:
        if field not in value:
            raise ValueError('Invalid curated input: ' + label + '.' + field + ' is required')


def _curated_sql(value, label, *, nullable=False):
    # TEXT columns historically accept SQLite scalar coercions. Preserve them,
    # and don't inspect values that add() skips before reaching SQL.
    if value is None and nullable:
        return
    if type(value) not in (str, int, float, bool):
        raise ValueError('Invalid curated input: ' + label + ' must be a SQLite scalar')


def _curated_entries(value, arity, label):
    # Preserve successful JSON iterable/unpacking behavior without changing raw
    # dictionaries. Typical inputs use arrays; dict keys and strings retain the
    # old iteration semantics where they supply the required tuple arity.
    if not isinstance(value, (list, dict, str)):
        raise ValueError('Invalid curated input: ' + label + ' must be iterable')
    for n, entry in enumerate(value):
        if not isinstance(entry, (list, dict, str)) or len(entry) != arity:
            raise ValueError('Invalid curated input: ' + label + '[' + str(n) + '] must have ' + str(arity) + ' fields')
        yield tuple(entry)


def _curated_row(row, source, label):
    _curated_required(row, ('key', 'brand', 'model', 'title', 'kind', 'affiliations'), label)
    _curated_sql(row['key'], label + '.key')
    _curated_sql(source['id'], label + '.source')
    if not isinstance(row['model'], str):
        raise ValueError('Invalid curated input: ' + label + '.model must be a string')
    for name in ('brand', 'title', 'kind'):
        _curated_sql(row[name], label + '.' + name, nullable=True)
    _curated_sql(row.get('url', source['url']), label + '.url')
    for company, role in _curated_entries(row['affiliations'], 2, label + '.affiliations'):
        if company:
            _curated_sql(company, label + '.affiliations.company')
            _curated_sql(role, label + '.affiliations.role', nullable=True)
    for kind, value in _curated_entries(row.get('identifiers', []), 2, label + '.identifiers'):
        if value:
            _curated_sql(kind, label + '.identifiers.kind', nullable=True)
    for name, value in _curated_entries(row.get('facts', []), 2, label + '.facts'):
        if value is not None and str(value):
            _curated_sql(name, label + '.facts.name', nullable=True)
    for entry in _curated_entries(row.get('hyperscaler', []), 3, label + '.hyperscaler'):
        for value in entry:
            _curated_sql(value, label + '.hyperscaler.value', nullable=True)


def prepare_curated(path):
    """Prepare all consumed rows and local snapshots before database mutation."""
    data = json.loads(Path(path).read_text())
    _curated_required(data, ('sources',), 'bundle')
    if not isinstance(data['sources'], list):
        raise ValueError('Invalid curated input: bundle.sources must be an array')
    if not data['sources']:
        return []  # Legacy no-op: even an absent/unused devices field is harmless.
    _curated_required(data, ('devices',), 'bundle')
    if not isinstance(data['devices'], list):
        raise ValueError('Invalid curated input: bundle.devices must be an array')
    for n, row in enumerate(data['devices']):
        _curated_required(row, ('source',), 'devices[' + str(n) + ']')
    prepared = []
    for n, source in enumerate(data['sources']):
        label = 'sources[' + str(n) + ']'
        _curated_required(source, ('id', 'url', 'tier', 'license', 'coverage'), label)
        _curated_sql(source['id'], label + '.id', nullable=True)
        for field in ('url', 'tier', 'license', 'coverage'):
            _curated_sql(source[field], label + '.' + field)
        snapshot = source.get('snapshot')
        if snapshot and not isinstance(snapshot, str):
            raise ValueError('Invalid curated input: ' + label + '.snapshot must be a path string')
        snapshot = Path(path).parent / snapshot if snapshot else None
        metadata = Catalog.prepare_snapshot(snapshot)
        rows = []
        for index, row in enumerate(data['devices']):
            if row['source'] == source['id']:
                _curated_row(row, source, 'devices[' + str(index) + ']')
                rows.append(row)
        prepared.append((source, snapshot, metadata, rows))
    return prepared


def curated(c,path):
    prepared = prepare_curated(path)
    if not prepared:
        return 0
    count = 0
    c.db.execute('SAVEPOINT devicehunt_curated_import')
    try:
        for source, snapshot, metadata, rows in prepared:
            sid = source['id']
            c.source(sid,source['url'],source['tier'],source['license'],source['coverage'],snapshot,
                     prepared_snapshot=metadata)
            for d in rows:
                c.add(sid,d['key'],d.get('url',source['url']),d['brand'],d['model'],d['title'],d['kind'],d,
                      d['affiliations'],d.get('identifiers',[]),d.get('facts',[]),d.get('hyperscaler',[]))
                count += 1
            # Keep this source boundary: removing orphans later changes which
            # occurrence supplies first-inserted identity title/kind metadata.
            c.finish(sid,len(rows),reindex=False,commit=False)
        c.reindex()
        c.db.execute('RELEASE devicehunt_curated_import')
    except BaseException as original:
        cleanup_errors = []
        try:
            c.db.execute('ROLLBACK TO devicehunt_curated_import')
        except BaseException as cleanup:
            cleanup_errors.append(cleanup)
        else:
            # Never release (and potentially commit) if rollback failed.
            try:
                c.db.execute('RELEASE devicehunt_curated_import')
            except BaseException as cleanup:
                cleanup_errors.append(cleanup)
        if cleanup_errors:
            try:
                original.curated_cleanup_errors = tuple(cleanup_errors)
                if hasattr(original, 'add_note'):
                    original.add_note('Curated savepoint cleanup failed; inspect curated_cleanup_errors before reusing the connection')
            except BaseException:
                pass  # A custom exception's diagnostic hooks cannot mask it.
        raise
    return count


def hardware_index(c,path,sid):
    if sid == 'oracle-eol':
        return oracle_eol(c, path)
    url=HARDWARE_INDEXES[sid]
    brand="Celestica" if sid.startswith("celestica-") else "Oracle"
    c.source(sid,url,"vendor",brand+" documentation; extracted factual product identifiers and links",
             "Explicit hardware product links from a vendor index. Software releases, navigation and third-party-branded listings are excluded. Does not establish physical OEM or exhaustive historical coverage.",path)
    entries={}
    for title,href in Page(Path(path).read_text()).links:
        model=title
        kind=None
        if sid=="celestica-storage":
            match=re.fullmatch(r"((?:SC|SD|SE)\d{4})(?: - Newest Addition)?",title)
            if match:
                model=match.group(1)
                kind="storage controller" if model.startswith("SC") else "storage enclosure"
        elif sid=="oracle-storage":
            if re.fullmatch(r"Oracle Storage Drive Enclosure DE\d-\d+[A-Z]",title):
                kind="storage enclosure"
            elif re.fullmatch(r"(?:Oracle|Sun) Flash Accelerator F\d+ PCIe Card",title):
                kind="flash accelerator"
        elif sid=="oracle-ethernet":
            if re.fullmatch(r"Oracle Switch ES\d-\d+",title):
                kind="switch"
            elif re.fullmatch(r"(?:Oracle|Sun) (?:Dual Port|Quad) .*(?:Adapter(?:, Mellanox)?|ExpressModule)",title):
                kind="network adapter"
        elif sid=="oracle-unified-storage":
            if re.fullmatch(r"Sun Storage \d{4} Unified Storage System Documentation",title):
                model=title.removesuffix(" Documentation")
                kind="storage appliance"
            elif re.fullmatch(r"Oracle ZFS Storage ZS\d-[A-Z]+ documentation",title):
                model=title.removesuffix(" documentation")
                kind="storage appliance"
        if kind:
            entries[(model,urljoin(url,href))]=(title,kind)
    if not entries:
        raise ValueError("No explicit hardware links found in "+sid)
    for (model,link),(title,kind) in entries.items():
        display=brand+" "+model if brand=="Celestica" else model
        query=re.sub(r"^(Oracle|Sun) ","",model)
        c.add(sid,model+"|"+link,link,brand,model,display,kind,{"title":title,"url":link,"index_url":url},
              [(brand,"brand" if brand=="Celestica" else "documentation_vendor")],
              [("model_search",query)],[("index_url",url)])
    c.finish(sid,len(entries))
    return len(entries)


ORACLE_GROUPS = {
    'Sun Netra CP2060/CP2080': ['Sun Netra CP2060', 'Sun Netra CP2080'],
    'Sun Netra t 1120/1125 Server': ['Sun Netra t 1120', 'Sun Netra t 1125'],
    'Sun Netra t 1400/1405 Server': ['Sun Netra t 1400', 'Sun Netra t 1405'],
    'Sun Netra t1 100/105 Server': ['Sun Netra t1 100', 'Sun Netra t1 105'],
}
ORACLE_IDENTITIES = {
    'Sun Datacenter InfiniBand Switch 36 (NM2-36P)': ('Sun Datacenter InfiniBand Switch 36', [('alias', 'NM2-36P')]),
    'Oracle Switch ES1-24 (OPUS R24)': ('Oracle Switch ES1-24', [('alias', 'OPUS R24')]),
    'Oracle Switch ES2-72 (OPUS R72)': ('Oracle Switch ES2-72', [('alias', 'OPUS R72')]),
    '32-Port Ethernet Switch Assembly (PUE2 - 4199516 - 7358381)': ('32-Port Ethernet Switch Assembly', [('alias', 'PUE2'), ('part_number', '4199516'), ('part_number', '7358381')]),
    'Oracle Netra SPARC S7-2 server': ('Netra SPARC S7-2', []),
}


def oracle_eol(c, path):
    url = HARDWARE_INDEXES['oracle-eol']
    rows = LinkedRows(Path(path).read_text()).linked_rows
    if sum(row == ['Product', 'Documentation', 'Next-Generation Product'] for row, _ in rows) != 1:
        raise ValueError('Missing or ambiguous Oracle retired-product table header')
    entries, skipped, scanned, section = [], [], 0, ''
    started = False
    for row, links in rows:
        if row == ['Product', 'Documentation', 'Next-Generation Product']:
            started = True
            continue
        if not started:
            continue
        if len(row) == 1:
            section = row[0]
            continue
        if len(row) != 3:
            raise ValueError('Unexpected Oracle product row shape')
        title, documentation, replacement = row
        scanned += 1
        if title.startswith('Fujitsu '):
            skipped.append(title)
            continue
        if not re.match(r'^(Oracle |Sun |SPARC |Netra |32-Port Ethernet Switch Assembly)', title):
            raise ValueError('Unexpected retired Oracle product: '+title)
        if '/' in title and title not in ORACLE_GROUPS:
            raise ValueError('Unreviewed grouped Oracle model: '+title)
        # Only the documentation cell supplies the documentation URL. A
        # successor link must never become evidence for the retired hardware.
        docs = [urljoin(url, href) for col, text, href in links if col == 1 and text == 'Documentation']
        for grouped in ORACLE_GROUPS.get(title, [title]):
            model = re.sub(r' Server(?: Module)?$', '', grouped)
            ids = [('model_search', model)]
            if title in ORACLE_IDENTITIES:
                model, reviewed_ids = ORACLE_IDENTITIES[title]
                ids += reviewed_ids
            facts = [('retired_product_label', title), ('vendor_category', section),
                     ('lifecycle_notice', 'Listed in Oracle end-of-life products index; no specific support deadline supplied')]
            if replacement:
                facts.append(('vendor_replacement_text', replacement))
            facts += [('documentation_url', doc) for doc in docs]
            entries.append((model, title, ids, facts, {'cells': row, 'cell_links': links, 'section': section}))
    if not entries:
        raise ValueError('Oracle retirement index contains no supported hardware rows')
    c.source('oracle-eol', url, 'vendor', 'Oracle factual product index; source rights retained',
             'All product rows in the cached retirement table scanned; Fujitsu-branded rows excluded. Explicit grouped Netra names expanded. Legacy Sun and Oracle catalog association is not physical OEM evidence. Replacement text is not an alias.', path)
    for model, title, ids, facts, raw in entries:
        c.add('oracle-eol', title+'|'+model, url, 'Oracle', model, model, 'retired hardware product', raw,
              [('Oracle', 'documentation_vendor')], ids, facts)
    c.finish('oracle-eol', scanned)
    return {'scanned': scanned, 'imported': len(entries), 'excluded': skipped}
