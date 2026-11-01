"""Exact, record-local buying evidence; no operability or entitlement inference."""
import shlex

from .catalog import target_for
from .specifications import source_fact_category, source_fact_statement

# Reviewed fact names, not value keywords. New/unrecognized fields remain visible
# in unclassified_facts until their semantics have been reviewed.
CATEGORY_FIELDS = {
    'power': set('power power_supply power_description power_statement_as_printed psu_rated_output_raw psu_configuration_1 psu_configuration_2 psu_configuration_3 power_configuration_limit power_limit'.split()),
    'cooling': {'cooling_description', 'compatibility_operating_temperature'},
    'chassis_physical': set('physical physical_description form_factor chassis_height chassis_model rack_format rack_configuration required_components compatibility_dimensions_in compatibility_dimensions_mm compatibility_weight compatibility_system_type'.split()),
    'firmware_boot_software': set('firmware label_observed_firmware ised_firmware_version software software_evidence software_installation_description recommended_firmware_as_printed compatibility_firmware_context compatibility_software compatibility_releases os_support_as_printed build_console_dev build_onie_arch build_uefi_enable boot_drive boot_media boot_storage guide_index_url guide_link_label compatibility_notes compatibility_note product_integration'.split()),
    'licensing': set('service_eligibility_scope service_existing_customer_access service_new_customer_access service_cloud_dependency service_notice service_end_date service_new_customer_end_date lifecycle_notice lifecycle_reason ocnos_hardware_end_of_life ocnos_hardware_end_of_sale'.split()),
    'compute_network_storage': set('architecture cpu cpus processor processor_sockets ram memory memory_as_printed memory_page_1_as_printed memory_page_2_as_printed memory_tested flash network networking ports ports_description interfaces interface_as_printed interface_requirement host_interface asic switch_asic switching_capacity storage storage_bays storage_connectivity storage_constraint drive_bays drive_support expander expansion expansion_ports io_modules controller chipset compute_configuration compute_platform_description gpu_as_printed motherboard reported_motherboard ocp_mezzanine system_bus compatibility_asic compatibility_capabilities compatibility_capacity compatibility_cpu compatibility_cpu_series compatibility_cpu_series_max_cores_per_socket compatibility_ports compatibility_ram compatibility_sockets'.split()),
}
for config in ('low', 'high', 'typical'):
    CATEGORY_FIELDS['power'].update('energystar_' + config + '_' + field for field in
                                   ('idle_watts', 'psu_count', 'psu_model', 'psu_rated_output_watts'))
    CATEGORY_FIELDS['compute_network_storage'].update('energystar_' + config + '_' + field for field in
                                                     ('installed_memory_gb', 'installed_processors', 'processor'))
CATEGORY_FIELDS['chassis_physical'].add('energystar_form_factor')
CATEGORY_FIELDS['firmware_boot_software'].add('energystar_operating_systems_supported')
CATEGORY_FIELDS['firmware_boot_software'].add('compatibility_bios_certification_rows')
CATEGORY_FIELDS['power'].add('label_observed_input_voltage_range')
CATEGORY_FIELDS['compute_network_storage'].update({'energystar_maximum_memory_capacity_(gb)', 'energystar_product_processor_socket_count'})
# Oracle's named SM variants stay separately labeled; processor wattage is a
# processor description, never a PSU/input/consumption statement.
for variant in ('sm_1u', 'sm_2u'):
    CATEGORY_FIELDS['chassis_physical'].add(variant + '_chassis_description')
    CATEGORY_FIELDS['compute_network_storage'].update(variant + '_' + field for field in
                                                     ('memory_description', 'pcie_description', 'drive_bays_description', 'processor_power_raw'))
# Published device-tree declarations retain their community source and full
# record context; these field names do not assert installed firmware or safety.
CATEGORY_FIELDS['firmware_boot_software'].update({'artifact_model_declaration', 'artifact_compatible_declaration',
                                                 'artifact_serial0_alias', 'artifact_partition_labels'})
# CMVP environments are dated software-module tested configurations; module
# versions/status/history/caveats remain full record context, not firmware claims.
CATEGORY_FIELDS['firmware_boot_software'].add('cmvp_tested_configuration')
LIMITS = [
    'Statements describe their own source record, not the configuration or condition of an auction unit.',
    'Seller statements remain seller claims; photographed firmware is an observation of that unit.',
    'Historical compatibility and source-code/document indexes do not establish bootability, download access, current support or license entitlement.',
    'Not collected means no statements from the reviewed fact-name mapping; inspect unclassified facts and source captures. It does not mean evidence is absent from all collected fields or that the unit cannot operate. No suffix decoding or family fallback is used.',
    'A collected category is not a complete buying assessment or readiness check.',
]
# specs --include-source-facts category -> buying-report category (read-only reuse of its whitelist).
SPECS_CATEGORY_TO_REPORT = {'power_description': 'power', 'cooling_description': 'cooling',
                            'physical_description': 'chassis_physical'}
IDENTIFIER_KINDS = {'alias', 'model_search', 'manufacturer_model', 'part_number'}


def run(db, query, vendor=None):
    wanted = query.strip().casefold()
    if not wanted:
        raise ValueError('Buying report requires a complete identifier')
    canonical = target_for(vendor) if vendor else None
    if vendor and not canonical:
        raise ValueError('Unknown target: ' + vendor)
    matches = {}
    for row in db.execute('SELECT r.id,d.model FROM records r JOIN devices d ON d.id=r.device_id'):
        if row['model'].strip().casefold() == wanted:
            matches.setdefault(row['id'], []).append({'kind': 'literal_model', 'value': row['model']})
    for row in db.execute('SELECT record_id,kind,value FROM identifiers ORDER BY record_id,kind,value'):
        if row['kind'] in IDENTIFIER_KINDS and row['value'].strip().casefold() == wanted:
            matches.setdefault(row['record_id'], []).append({'kind': row['kind'], 'value': row['value']})
    categories = {name: {'status': 'not_collected', 'status_scope': 'reviewed fact-name mapping only; inspect unclassified_facts and captures', 'statements': []} for name in CATEGORY_FIELDS}
    records, unclassified = [], []
    for rid in sorted(matches):
        roles = [dict(r) for r in db.execute('SELECT company,target,role FROM affiliations WHERE record_id=? ORDER BY company,role', (rid,))]
        if canonical and not any(r['target'] == canonical for r in roles):
            continue
        row = db.execute('SELECT r.id AS record_id,r.device_id,d.brand,d.model,d.revision,r.source_id,r.source_url,r.observed_at,s.url AS source_home_url,s.tier,s.sha256,s.coverage,s.fetched_at,s.upstream_revision FROM records r JOIN devices d ON d.id=r.device_id JOIN sources s ON s.id=r.source_id WHERE r.id=?', (rid,)).fetchone()
        context = [dict(r) for r in db.execute('SELECT name,value FROM facts WHERE record_id=? ORDER BY name,value', (rid,))]
        record = dict(row, company_roles=roles, matched_identifiers=matches[rid], context_facts=context)
        record['claim_status'] = 'seller_claim' if row['tier'] == 'marketplace' else 'source_statement'
        records.append(record)
        for fact in context:
            statement = {k: v for k, v in record.items() if k not in ('context_facts', 'matched_identifiers')}
            statement.update(fact_name=fact['name'], statement=fact['value'], context_record_id=rid)
            classified = False
            for name, fields in CATEGORY_FIELDS.items():
                if fact['name'] in fields:
                    categories[name]['statements'].append(statement)
                    classified = True
            if not classified:
                unclassified.append(statement)
    for category in categories.values():
        if category['statements']:
            category['status'] = ('seller_only' if all(s['claim_status'] == 'seller_claim' for s in category['statements'])
                                  else 'collected_source_statements')
    add_source_fact_hints(categories, records, query, canonical)
    return {'query': query, 'vendor': canonical, 'match_status': 'exact_records_found' if records else 'no_exact_records',
            'matched_records': records, 'categories': categories, 'unclassified_facts': unclassified, 'limitations': LIMITS}


def specs_command(records, query, vendor=None):
    """specs invocation(s) reproducing the report's identity and vendor scope.

    Part-number-only matches need --exact-part-number (--exact-model ignores part numbers);
    every other matched kind is covered by --exact-model. A mix needs both (their union).
    """
    pn_only = any(all(m['kind'] == 'part_number' for m in r['matched_identifiers']) for r in records)
    other = any(any(m['kind'] != 'part_number' for m in r['matched_identifiers']) for r in records)
    tail = ' --include-source-facts' + (' --vendor ' + shlex.quote(vendor) if vendor else '')
    base = 'python3 -m devicehunt specs ' + shlex.quote(query)
    commands = []
    if other or not pn_only:
        commands.append(base + ' --exact-model' + tail)
    if pn_only:
        commands.append(base + ' --exact-part-number' + tail)
    return ' ; '.join(commands)


def add_source_fact_hints(categories, records, query, vendor=None):
    """Point not_collected categories at whitelisted `specs --include-source-facts` facts.

    Uses only the matched records (same identity/vendor scope as the report); status is untouched.
    N counts distinct (source, fact name, statement) rows, mirroring specs' de-duplication.
    """
    found = {}
    for record in records:
        for fact in record['context_facts']:
            category = SPECS_CATEGORY_TO_REPORT.get(source_fact_category(fact['name']))
            if category and categories[category]['status'] == 'not_collected' \
                    and source_fact_statement(fact['name'], fact['value']) is not None:
                found.setdefault(category, set()).add((record['source_id'], fact['name'], fact['value']))
    see = specs_command(records, query, vendor)
    for category, rows in found.items():
        categories[category]['source_facts_available'] = {
            'count': len(rows), 'fact_names': sorted({name for _, name, _ in rows}), 'see': see}


def render_table(report):
    """Readable record contexts once; JSON statements reference their context_record_id."""
    lines = [f"buying report: {report['query']} ({report['match_status']})"]
    lines.extend(report['limitations'])
    for name, category in report['categories'].items():
        lines.append(f"\n{name}: {category['status']}")
        for statement in category['statements']:
            lines.append(f"  [{statement['record_id']}; {statement['claim_status']}] {statement['fact_name']}: {statement['statement']}")
        hint = category.get('source_facts_available')
        if hint:
            lines.append(f"  source_facts_available: {hint['count']} (names: {', '.join(hint['fact_names'])}; see: {hint['see']})")
    for record in report['matched_records']:
        lines.append(f"\nrecord {record['record_id']}: {record['brand']} {record['model']} revision={record['revision']!r}")
        for key in ('source_id', 'source_url', 'source_home_url', 'tier', 'sha256', 'coverage', 'observed_at', 'fetched_at', 'upstream_revision'):
            lines.append(f"  {key}: {record[key]}")
        lines.append(f"  company_roles: {record['company_roles']}")
        lines.append(f"  matched_identifiers: {record['matched_identifiers']}")
        lines.append('  full record context (includes unclassified facts):')
        lines.extend(f"    {f['name']}: {f['value']}" for f in record['context_facts'])
    if not report['matched_records']:
        lines.append('\nno exact evidence records collected for this query/vendor; no variant specifications borrowed.')
    return '\n'.join(lines)
