"""Retain scoped vendor statements; never decode ordering-code suffixes."""
import re
from html.parser import HTMLParser

from .catalog import target_for

VOID = {'area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr'}
CATEGORIES = {
    'power_description': re.compile(r'\b(?:PSUs?|power suppl\w*|power input|power consumption|power budget|voltage|watts?|AC|DC|PoE\w*)\b|\b\d+\s*W\b', re.I),
    'cooling_description': re.compile(r'\b(?:airflow|air flow|fans?|cooling|hot/cold aisle)\b', re.I),
    'software_installation_description': re.compile(r'\b(?:ONIE|pre-loaded|preloaded|Network Operating System)\b', re.I),
    'physical_description': re.compile(r'\b(?:\d+\s*(?:RU|rack units?)|rack.mount\w*|form factor)\b', re.I),
}

# Opt-in (`--include-source-facts`) whitelist of practical source-fact NAMES, each fullmatch-ed
# and mapped to one of the four categories above. Derived from the names actually present in
# data/devices.sqlite (SELECT name,COUNT(*) FROM device_facts GROUP BY name), reviewed 2026-10-11.
# Add a name only after reading its values; never widen with substring/keyword patterns.
# Deliberately excluded: software-reference declarations (sonic_*, pddf_*, stratum_*, em_*, dts_*,
# nos_*, obmc_*, fbobmc_*: counts/names read from NOS or BMC source trees describing code, not a
# hardware SKU), devicecode `power`/`power_supply` (JSON adapter-schema blobs), CPU power limits,
# power_limit/power_configuration_limit (disclaimer sentences, not facts), energystar_archive_* test-config noise, redhat_cert_detail_form_factors (system class like
# "Server", not a chassis form factor) and node_*/server_node_model identity facts.
SOURCE_FACT_NAMES = (
    # SPEC submissions: PSU quantity/rating/model strings as reported by the tester.
    ('power_description', r'spec_(?:shared_)?power_supply(?:_details|_quantity_and_rating_w)?'),
    # NetBox library: power ports with maximum_draw, and port count (library entries, not measurements).
    ('power_description', r'netbox_power_ports|netbox_power_port_count'),
    # ENERGY STAR summary facts: PSU count/model/rated output and measured idle watts per config.
    ('power_description', r'energystar_(?:high|low|typical)_(?:psu_count|psu_model|psu_rated_output_watts|idle_watts)'),
    # Vendor datasheet facts kept as printed: PSU configuration, redundancy, rating and part numbers.
    ('power_description', r'psu_configuration_[123]|psu_rated_output_raw|psu_model_as_printed|psu_redundancy_as_printed'
                          r'|psu_interchangeability_as_printed'
                          r'|power_statement_as_printed|power_redundancy_statement_as_printed|power_supply_note_as_printed'
                          r'|power_supply_part_numbers_as_printed|power_supply_fan_tray_part_numbers_as_printed'
                          r'|back_panel_power_supplies_as_printed'),
    # Airflow direction and fan modules/service notes (NetBox airflow; vendor datasheet fan facts).
    ('cooling_description', r'netbox_airflow|failed_psu_slot_airflow_note_as_printed|fan_service_constraint_as_printed'
                            r'|back_panel_fan_modules_as_printed|rear_fan_part_numbers_as_printed'),
    # Rack units, depth, weight and form factor (NetBox, SPEC, ENERGY STAR, catalogs, HCLs).
    ('physical_description', r'netbox_u_height|netbox_is_full_depth|netbox_weight|form_factor|spec_form_factor'
                             r'|spec_shared_form_factor|energystar_form_factor|chassis_height(?:_as_printed)?'
                             r'|chassis_dimensions|compatibility_dimensions_(?:in|mm)|compatibility_weight'
                             r'|sm_[12]u_chassis_description'),
    # DMI chassis type enumerations (linuxhw dumps; often "Desktop"/"Other" on servers: a label, not a spec).
    ('physical_description', r'dmi_chassis_type'),
    # SPEC node counts: nodes per system / identical nodes / shared enclosure of a multi-node submission.
    ('physical_description', r'spec_nodes_per_system|spec_number_of_identical_nodes|spec_shared_enclosure'),
)
SOURCE_FACT_PATTERNS = tuple((cat, re.compile(pat)) for cat, pat in SOURCE_FACT_NAMES)


# Values that carry no information; such source-fact rows are dropped, never shown.
PLACEHOLDER_VALUES = frozenset({'', '-', '--', '---', '—', '–', 'n/a', 'na', 'n.a.', 'none', 'null', 'nil', 'unknown', 'tbd', 'tba', '?'})
ENERGYSTAR_IDLE = re.compile(r'energystar_(high|low|typical)_idle_watts')
ENERGYSTAR_CONFIG = {'high': 'high-end performance', 'low': 'low-end performance', 'typical': 'typical or single'}
COMPONENT_FORM_FACTOR = re.compile(r'\b(?:PCIe|PCI Express|OCP|card|module|mezzanine)\b', re.I)


# Human labels for whitelisted fact names. Source values are often bare ("1", "2U", "1 x 650"), so a
# source-fact statement is shown as "<label>: <raw value>". Every whitelisted name needs a label here
# (a test pins that); the ENERGY STAR idle-watts and component form-factor cases keep their own wording.
SOURCE_FACT_LABELS = {
    'spec_power_supply': 'PSU quantity and rating as submitted to SPEC',
    'spec_power_supply_details': 'PSU details as submitted to SPEC',
    'spec_power_supply_quantity_and_rating_w': 'PSU quantity and rating (W)',
    'spec_shared_power_supply_details': 'shared-enclosure PSU details as submitted to SPEC',
    'spec_shared_power_supply_quantity_and_rating_w': 'shared-enclosure PSU quantity and rating (W)',
    'netbox_power_ports': 'NetBox power port maximum draw entry (W, library entry; not PSU capacity or measured draw)',
    'netbox_power_port_count': 'NetBox power port count (library entry)',
    'energystar_high_psu_count': 'ENERGY STAR high-end performance configuration PSU count',
    'energystar_low_psu_count': 'ENERGY STAR low-end performance configuration PSU count',
    'energystar_typical_psu_count': 'ENERGY STAR typical or single configuration PSU count',
    'energystar_high_psu_model': 'ENERGY STAR high-end performance configuration PSU model',
    'energystar_low_psu_model': 'ENERGY STAR low-end performance configuration PSU model',
    'energystar_typical_psu_model': 'ENERGY STAR typical or single configuration PSU model',
    'energystar_high_psu_rated_output_watts': 'ENERGY STAR high-end performance configuration PSU rated output (W)',
    'energystar_low_psu_rated_output_watts': 'ENERGY STAR low-end performance configuration PSU rated output (W)',
    'energystar_typical_psu_rated_output_watts': 'ENERGY STAR typical or single configuration PSU rated output (W)',
    'psu_configuration_1': 'PSU configuration (as printed)',
    'psu_configuration_2': 'PSU configuration (as printed)',
    'psu_configuration_3': 'PSU configuration (as printed)',
    'psu_rated_output_raw': 'PSU rated output (as printed)',
    'psu_model_as_printed': 'PSU model (as printed)',
    'psu_redundancy_as_printed': 'PSU redundancy (as printed)',
    'psu_interchangeability_as_printed': 'PSU interchangeability (as printed)',
    'power_statement_as_printed': 'power statement (as printed)',
    'power_redundancy_statement_as_printed': 'power redundancy statement (as printed)',
    'power_supply_note_as_printed': 'power supply note (as printed)',
    'power_supply_part_numbers_as_printed': 'power supply part numbers (as printed)',
    'power_supply_fan_tray_part_numbers_as_printed': 'power supply fan tray part numbers (as printed)',
    'back_panel_power_supplies_as_printed': 'back panel power supplies (as printed)',
    'netbox_airflow': 'NetBox airflow (library entry)',
    'failed_psu_slot_airflow_note_as_printed': 'failed PSU slot airflow note (as printed)',
    'fan_service_constraint_as_printed': 'fan service constraint (as printed)',
    'back_panel_fan_modules_as_printed': 'back panel fan modules (as printed)',
    'rear_fan_part_numbers_as_printed': 'rear fan part numbers (as printed)',
    'netbox_u_height': 'NetBox rack height (U, library entry)',
    'netbox_is_full_depth': 'NetBox full-depth flag (library entry)',
    'netbox_weight': 'NetBox weight (library entry)',
    'form_factor': 'form factor',
    'spec_form_factor': 'form factor as submitted to SPEC',
    'spec_shared_form_factor': 'shared-enclosure form factor as submitted to SPEC',
    'energystar_form_factor': 'form factor as listed by ENERGY STAR',
    'chassis_height': 'chassis height',
    'chassis_height_as_printed': 'chassis height (as printed)',
    'chassis_dimensions': 'chassis dimensions',
    'compatibility_dimensions_in': 'dimensions (in, compatibility list)',
    'compatibility_dimensions_mm': 'dimensions (mm, compatibility list)',
    'compatibility_weight': 'weight (compatibility list)',
    'sm_1u_chassis_description': 'chassis description (1U)',
    'sm_2u_chassis_description': 'chassis description (2U)',
    'dmi_chassis_type': 'chassis type reported by firmware in submitted dmidecode dumps',
    'spec_nodes_per_system': 'nodes per system as submitted to SPEC',
    'spec_number_of_identical_nodes': 'number of identical nodes as submitted to SPEC',
    'spec_shared_enclosure': 'shared enclosure as submitted to SPEC',
}
# linuxhw DMI values end in a citation path "(Server/Vendor/Family/Model/<12 hex dump id>:<n>)"; the
# same path stays in source_url, so the displayed statement drops it. A "[N of M dumps]" tail is kept.
DMI_CITATION = re.compile(r'\s*\([^()]*/[0-9A-Fa-f]{12}:\d+\)')


def source_fact_statement(name, value):
    """Return the statement to show for a whitelisted fact, or None for placeholders."""
    if value.strip().casefold() in PLACEHOLDER_VALUES:
        return None
    match = ENERGYSTAR_IDLE.fullmatch(name)
    if match:  # bare number in the source; say what it is
        return 'idle power as measured in the ENERGY STAR %s test configuration: %s W' % (ENERGYSTAR_CONFIG[match[1]], value.strip())
    if name == 'form_factor' and COMPONENT_FORM_FACTOR.search(value):
        return 'component form factor (not a chassis): ' + value
    if name == 'dmi_chassis_type':
        value = DMI_CITATION.sub('', value).strip()
    label = SOURCE_FACT_LABELS.get(name)
    return '%s: %s' % (label, value) if label else value


def source_fact_category(name):
    for category, pattern in SOURCE_FACT_PATTERNS:
        if pattern.fullmatch(name):
            return category
    return None


class FeatureSection(HTMLParser):
    def __init__(self, body):
        super().__init__(convert_charrefs=True)
        self.stack, self.blocks, self.parts = [], [], []
        self.block_depth = None
        self.sections = 0
        self.feed(body)
        if self.stack:
            raise ValueError('Unclosed product features section')

    def handle_starttag(self, tag, attrs):
        if dict(attrs).get('id') == 'features':
            self.sections += 1
            if self.stack or tag in VOID:
                raise ValueError('Nested or invalid product features section')
            self.stack = [tag]
        elif self.stack and tag not in VOID:
            self.stack.append(tag)
        if self.stack:
            if tag in {'li', 'p'} and self.block_depth is None:
                self.block_depth, self.parts = len(self.stack), []
            elif tag == 'br' and self.block_depth is not None:
                self.parts.append(' ')

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if tag not in VOID:
            self.handle_endtag(tag)

    def handle_data(self, data):
        if self.block_depth is not None and not {'script', 'style'} & set(self.stack):
            self.parts.append(data)

    def handle_endtag(self, tag):
        if tag not in self.stack:
            return
        index = len(self.stack) - 1 - self.stack[::-1].index(tag)
        self.stack = self.stack[:index]
        if self.block_depth is not None and len(self.stack) < self.block_depth:
            text = ' '.join(''.join(self.parts).split())
            if text:
                self.blocks.append(text)
            self.parts, self.block_depth = [], None


def product_statements(body):
    facts = set()
    for text in FeatureSection(body).blocks:
        # Preserve whole statements, including alternatives and qualifications.
        # Unexpectedly large blocks need parser review instead of silent truncation.
        matches = [name for name, pattern in CATEGORIES.items() if pattern.search(text)]
        if matches and len(text) > 4000:
            raise ValueError('Oversized product specification block')
        facts.update((name, text) for name in matches)
    return sorted(facts)


def search(catalog, query='', target=None, limit=100, exact_model=False, exact_part_number=False,
           include_source_facts=False):
    if limit < -1:
        raise ValueError('Limit must be -1 or nonnegative')
    if exact_model and exact_part_number:
        raise ValueError('Choose exact model or exact part number')
    if exact_part_number:
        return part_number_statements(catalog, query, target, limit, include_source_facts)
    canonical = target_for(target) if target else None
    if target and canonical is None:
        raise ValueError('Unknown target: ' + target)
    if exact_model:
        if not query.strip():
            raise ValueError('Exact model lookup requires a model string')
        wanted = query.strip().casefold()
        # Literal identities work independently of the discovery index. An alias
        # qualifies only its own record, not every configuration of that device.
        records = {r['id'] for r in catalog.db.execute(
            'SELECT r.id,d.model FROM records r JOIN devices d ON d.id=r.device_id')
            if r['model'].strip().casefold() == wanted}
        records.update(r['record_id'] for r in catalog.db.execute(
            "SELECT i.record_id,i.value FROM identifiers i "
            "WHERE i.kind IN ('alias','model_search','manufacturer_model')")
            if r['value'].strip().casefold() == wanted)
        rows = [r for r in statement_rows(catalog, canonical, include_source_facts) if r['record_id'] in records]
    else:
        ids = {d['id'] for d in catalog.search(query, target=target, limit=-1)}
        rows = [r for r in statement_rows(catalog, canonical, include_source_facts) if r['device_id'] in ids]
    return rows if limit == -1 else rows[:limit]


def statement_rows(catalog, target=None, include_source_facts=False):
    """Keep each statement's company filter and provenance on its own record.

    With include_source_facts, whitelisted source facts join the curated statements; every row then
    gains `origin` ('curated statement' or 'source fact') and `source_fact_name`.
    """
    rows = curated_statement_rows(catalog, target)
    if not include_source_facts:
        return rows
    for row in rows:
        row.update(origin='curated statement', source_fact_name='')
    names = {}
    for (name,) in catalog.db.execute('SELECT DISTINCT name FROM facts'):
        category = source_fact_category(name)
        if category:
            names[name] = category
    if names:
        clause = ''
        args = list(names)
        if target:
            clause = ' AND EXISTS (SELECT 1 FROM affiliations a WHERE a.record_id=r.id AND a.target=?)'
            args.append(target)
        for r in catalog.db.execute(
                'SELECT r.id AS record_id,d.id AS device_id,d.brand,d.model,d.revision,'
                'f.name AS fact_name,f.value AS statement,r.source_id,r.source_url,s.tier '
                'FROM facts f JOIN records r ON r.id=f.record_id '
                'JOIN devices d ON d.id=r.device_id JOIN sources s ON s.id=r.source_id '
                'WHERE f.name IN (' + ','.join('?' for _ in names) + ')' + clause, args):
            row = dict(r)
            name = row.pop('fact_name')
            row['statement'] = source_fact_statement(name, row['statement'])
            if row['statement'] is None:
                continue
            rows.append({k: row[k] for k in ('record_id', 'device_id', 'brand', 'model', 'revision')} |
                        {'category': names[name], 'statement': row['statement'], 'source_id': row['source_id'],
                         'source_url': row['source_url'], 'tier': row['tier'],
                         'origin': 'source fact', 'source_fact_name': name})
    rows.sort(key=lambda r: (r['brand'], r['model'], r['revision'], r['device_id'], r['source_id'], r['record_id'],
                             r['category'], r['origin'] != 'curated statement', r['source_fact_name'], r['statement']))
    return dedupe_source_facts(rows)


def dedupe_source_facts(rows):
    """Drop repeated source-fact rows; curated rows are never touched.

    Rows identical in (device_id, category, statement, source_fact_name, source_id) collapse to the
    first one (lowest record_id in sort order). Different sources stay separate for attribution.
    """
    seen = set()
    kept = []
    for row in rows:
        if row['origin'] == 'source fact':
            key = (row['device_id'], row['category'], row['statement'], row['source_fact_name'], row['source_id'])
            if key in seen:
                continue
            seen.add(key)
        kept.append(row)
    return kept


def curated_statement_rows(catalog, target=None):
    clause = ''
    args = list(CATEGORIES)
    if target:
        clause = ' AND EXISTS (SELECT 1 FROM affiliations a WHERE a.record_id=r.id AND a.target=?)'
        args.append(target)
    return [dict(r) for r in catalog.db.execute(
        'SELECT r.id AS record_id,d.id AS device_id,d.brand,d.model,d.revision,'
        'f.name AS category,f.value AS statement,r.source_id,r.source_url,s.tier '
        'FROM facts f JOIN records r ON r.id=f.record_id '
        'JOIN devices d ON d.id=r.device_id JOIN sources s ON s.id=r.source_id '
        'WHERE f.name IN (' + ','.join('?' for _ in CATEGORIES) + ')' + clause +
        ' ORDER BY d.brand,d.model,d.revision,d.id,r.source_id,r.id,f.name,f.value', args)]


def part_number_statements(catalog, query, target, limit, include_source_facts=False):
    """Keep a part number, its vendor association and facts on the same record."""
    if not query.strip():
        raise ValueError('Exact part-number lookup requires a part number')
    wanted = query.strip().casefold()
    args = []
    clause = "i.kind='part_number'"
    if target:
        canonical = target_for(target)
        if canonical is None:
            raise ValueError('Unknown target: ' + target)
        clause += ' AND EXISTS (SELECT 1 FROM affiliations a WHERE a.record_id=i.record_id AND a.target=?)'
        args.append(canonical)
    matched = {}
    for item in catalog.db.execute('SELECT i.record_id,i.value FROM identifiers i WHERE ' + clause +
                                    ' ORDER BY i.record_id,i.value', args):
        if item['value'].strip().casefold() == wanted:
            matched.setdefault(item['record_id'], []).append(item['value'])
    if not matched:
        return []
    rows = []
    for item in statement_rows(catalog, None, include_source_facts):
        if item['record_id'] in matched:
            rows.append(dict(item, matched_part_numbers=matched[item['record_id']]))
    return rows if limit == -1 else rows[:limit]
