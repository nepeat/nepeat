"""Read-only literal search across catalog identities and cached raw registries."""
import json

from .validation import IDENTITY_KINDS


REGISTRIES = [
    ('inspur_support', 'inspur_support_entries', 'inspur-support-models', 'public vendor support-menu entry',
     ['product_id', 'parent_id', 'name', 'raw_json'], 'source_id,row_number',
     'source_id,row_number,product_id,parent_id,name,level_raw,leaf_raw,raw_json,source_url',
     'Includes systems, third-party parts, software and grouped names. Support-menu evidence only; '
     'no automatic device identity, brand, manufacturer, operator or model-equivalence claim. '
     'Duplicates and source category IDs remain raw; complete historical coverage is unproven.'),
    ('fcc_sitemaps', 'fcc_sitemap_urls', 'fcc-sitemaps', 'public sitemap URL row',
     ['fcc_id','record_url','sitemap_url'], 'source_id,sitemap_url,ordinal',
     'source_id,sitemap_url,ordinal,record_url,fcc_id,page_kind,lastmod_raw,changefreq_raw,priority_raw,record_url AS source_url',
     'Discovery URLs only. Sitemap lastmod is site metadata, not a grant date. '
     'No model, grantee, manufacturer or deployment assertion; duplicate rows retained.'),
    ('fcc_company', 'fcc_company_rows', ['fcc-company-accton', 'fcc-company-amazon'], 'reviewed FCC company-table row',
     ['company_name','company_frn','fcc_id','listed_date','model_cell','application_purpose','frequency_raw'],
     'source_id,row_number',
     'source_id,row_number,page_number,page_row_number,company_name,company_frn,fcc_id,listed_date,model_cell,application_purpose,frequency_raw,source_url,web_text_line',
     'Factual web-tool table extracts, not original HTML or complete FCC history. Duplicate rows retained. '
     'Company FRN is page-level metadata. Model cells and frequencies remain unverified mirror strings; '
     'no device-model alias, application/grant, manufacturer or operator claim inferred.'),
    ('japan', 'japan_equipment',
     ['japan-mic-mirror-'+key for key in ('accton','edgecore','edge-core','celestica','oracle','amazon')],
     'Japanese mirror approval/class row',
     ['approval_type','approval_number','approval_date_raw','registered_name','equipment_class','model_field','cells_json'],
     'source_id,row_number',
     'source_id,row_number,approval_type,approval_number,approval_date_raw,registered_name,equipment_class,model_field,detail_url,cells_json,source_url',
     'Selected Lang-ship company-name queries, not the complete MIC database. Registered name is not '
     'inferred manufacturer. Grouped models, class rows and duplicates retained. The mirror reports '
     'width normalization; no automatic model, factory or current certification assertion.'),
    ('public_registers', 'public_equipment_rows',
     ['public-register-ntrc-vc', 'public-register-mauritius-icta'], 'published approval-table row',
     ['approval_number', 'brand', 'make', 'device_description', 'model', 'foreign_certification', 'cells_json'],
     'source_id,row_number',
     'source_id,row_number,approval_number,brand,make,device_description,model,foreign_certification,cells_json,source_url',
     'NTRC Saint Vincent brand and ICTA Mauritius make are separate source fields, not manufacturer claims. '
     'Grouped models, duplicate rows and blank cells retained; no catalog identity, operator or current approval validity inferred. '
     'Foreign certification strings are unverified references. Consult per-source coverage.'),
    ('ghana', 'ghana_equipment', 'ghana-nca-equipment', 'Ghana NCA public equipment row',
     ['approval_number', 'manufacturer', 'brand', 'product_type', 'model', 'cells_json'],
     'source_id,row_number',
     'source_id,row_number,detail_id,approval_number,manufacturer,brand,product_type,model,cells_json,detail_url,source_url',
     'Public table snapshot; historical/current completeness and publication date unverified. '
     'Blank approval numbers and full labels retained. Detail links do not imply fetched certificates. '
     'Manufacturer and brand remain separate; no factory, deployment or model equivalence inferred.'),
    ('nigeria', 'nigeria_equipment', 'nigeria-ncc-equipment', 'Nigeria NCC published CSV row',
     ['applicant', 'certificate_holder', 'equipment_name', 'models', 'manufacturer', 'cells_json'],
     'source_id,row_number',
     'source_id,row_number,serial_number,applicant,certificate_holder,equipment_name,models,manufacturer,cells_json,invalid_cell_bytes_json,source_url',
     'Published register with separate manufacturer, holder and applicant claims. Row numbers are not certificate '
     'IDs. Grouped models and damaged source text remain raw; flagged invalid cell bytes are retained as base64. '
     'No current validity, complete historical coverage, factory or deployment inference.'),
    ('ised', 'ised_equipment', 'ised-rel-25869', 'Canadian REL query CSV row',
     ['company_number', 'company_name', 'hvin', 'product_marketing_name', 'host_marketing_name',
      'firmware_version', 'certification_number', 'cells_json'], 'source_id,row_number',
     'source_id,row_number,company_number,company_name,hvin,product_marketing_name,host_marketing_name,firmware_version,certification_number,approval_timestamp_raw,matches_html_identity,cells_json,source_url',
     'Complete CSV exports from selected company queries, not the complete REL. Radio configurations and duplicate '
     'rows remain separate, including rows with no exact labelled-HTML identity match. Company is a regulatory '
     'association, not manufacturer/deployment proof; all 44 raw cells are retained.'),
    ('energystar_datacenter', 'energystar_datacenter',
     ['energystar-' + kind for kind in ('network', 'storage-file', 'storage-block', 'storage-file-v1', 'storage-block-v1')],
     'ENERGY STAR network/storage qualification row',
     ['product_id', 'partner', 'brand', 'model_name', 'model_number', 'model_identifier', 'raw_json'],
     'source_id,product_id',
     'source_id,dataset_kind,product_id,partner,brand,model_name,model_number,model_identifier,certified_date_raw,available_date_raw,raw_json,source_url,sheet,row_number',
     'All-brand published network/storage lists and historical storage files; configurations remain separate. '
     'Program partners, component makers and controller names do not establish chassis manufacturers. '
     'Storage performance-per-watt ratios are not power-consumption measurements.'),
    ('energystar_server_archives', 'energystar_server_archives',
     ['energystar-server-archive-v2', 'energystar-server-archive-v1_1'], 'historical ENERGY STAR qualification row',
     ['partner', 'brand', 'model_name', 'model_number', 'model_identifier', 'fields_json'],
     'source_id,sheet,row_number',
     'source_id,sheet,row_number,partner,brand,model_name,model_number,model_identifier,certified_date_raw,available_date_raw,fields_json,cells_json,source_url',
     'Historical qualification lists with original workbook/sheet/row provenance. Family configurations remain separate. '
     'Partner and brand are not manufacturer claims; power fields retain test configuration and voltage.'),
    ('energystar_servers', 'energystar_servers', 'energystar-enterprise-servers', 'ENERGY STAR server record',
     ['product_id', 'partner', 'brand', 'model_name', 'model_number', 'model_identifier', 'raw_json'],
     'source_id,product_id',
     'source_id,product_id,partner,brand,model_name,model_number,model_identifier,certified_date_raw,available_date_raw,raw_json,source_url',
     'Published dataset snapshot including all brands. Brand/program partner are not manufacturer claims. '
     'Raw fields preserve separate test configurations; idle results and PSU ratings do not predict auction-unit consumption.'),
    ('rra', 'rra_equipment', 'rra-official', 'official Korean approval detail',
     ['applicant', 'equipment_name', 'main_model', 'derived_models', 'certificate_number', 'manufacturer', 'other_raw'],
     'source_id,row_number',
     'source_id,row_number,applicant,equipment_name,main_model,derived_models,certificate_number,manufacturer,country_raw,approval_date_raw,other_raw,cells_json,source_url,detail_sha256',
     'Complete returned manufacturer-query CSV with original labelled detail pages, not all Korean approvals. '
     'Composite company cells stay unsplit; raw dates/countries do not establish current validity or factories.'),
    ('fcc_details', 'fcc_detail_pages', 'fcc-detail-pages', 'mirror detail page attempt',
     ['fcc_id', 'grantee_code', 'grantee_name', 'product_description', 'fields_json', 'status', 'issue'],
     'fcc_id,source_id', 'source_id,fcc_id,grantee_name,product_description,fields_json,source_url,status,issue',
     'Indexed detail-page attempts and selected supplemental probes, not every application. '
     'Dates remain verbatim; unavailable responses stay explicit and do not prove that an FCC filing is absent.'),
    ('fcc_excerpts', 'fcc_excerpt_text', 'fcc-detail-pages', 'mirror exhibit-text excerpt',
     ['fcc_id', 'grantee_name', 'product_description', 'category', 'text'],
     'fcc_id,ordinal', 'source_id,fcc_id,ordinal,grantee_name,category,text,truncated,source_url',
     'Mirror-supplied text, sometimes explicitly truncated. Not an archived PDF, model assertion, or complete document. '
     'No document ID or page number inferred. Returned excerpt is shortened; SQL retains full cached text.'),
    ('documents', 'document_text', 'fcc-documents-2avpl-te1a001', 'cached PDF page',
     ['fcc_id', 'document_id', 'title', 'category', 'text'],
     'fcc_id,document_id,page_number',
     'source_id,document_key,fcc_id,document_id,title,category,source_url,pdf_file,pdf_sha256,page_number,text_status',
     'Metadata or embedded page-text match; no OCR or automatic identity inference. '
     'Use documents for text excerpts. Image-only pages remain available in the PDF.'),
    ('fcc_ids', 'fcc_id_index', ['fcc-index-' + code for code in ('hed', 'yzk', 'a4h', '2avpl', 'mrf', 'nvn')],
     'mirror FCC ID index entry',
     ['fcc_id', 'grantee_code', 'grantee_name', 'product_description', 'equipment_class', 'application_purpose'],
     'grantee_name,fcc_id,source_id',
     'source_id,fcc_id,grantee_code,grantee_name,listed_date,product_description,equipment_class,application_purpose,source_url,page_url',
     'Selected grantee indexes from incomplete mirrors, including labelled factual table extracts, not all FCC applications or device models. '
     'Listed dates are verbatim mirror dates, not verified grant dates. Not-found company pages are coverage gaps.'),
    ('pci', 'pci_entries', 'pci-id-repository', 'PCI name entry',
     ['id_path', 'name', 'vendor_name', 'subvendor_name', 'comments'], 'entry_kind,id_path',
     'source_id,line_number,entry_kind,id_path,name,vendor_id,device_id,subvendor_id,subdevice_id,class_id,subclass_id,interface_id,vendor_name,subvendor_name,comments',
     'Community PCI function/subsystem/class names; no chassis, manufacturer, operator or unit attribution. Vendor namespaces remain separate.'),
    ('ieee', 'ieee_assignments', 'ieee-mac-assignments', 'address assignment row',
     ['registry', 'assignment', 'organization_name', 'organization_address'],
     'organization_name,registry,assignment,row_number',
     'source_id,registry,row_number,assignment,prefix_bits,organization_name,organization_address,source_url',
     'Registered address-block organizations, not models, factories or operators. Names and duplicate assignments remain verbatim.'),
    ('ntra', 'ntra_equipment', 'ntra-register-2026-01', 'PDF table row',
     ['product_description', 'brand', 'model', 'linked_models', 'registry_number', 'cells_json'],
     'source_id,page,table_number,row_number',
     'source_id,page,table_number,row_number,registry_number,product_description,brand,model,linked_models,parse_status,cells_json,source_url',
     'Registry brand only, not manufacturer or operator. Historical combined brand/model cells stay unsplit in raw_cells. '
     'Linked models remain separate; review-required rows retain raw cells. Snapshot rows are not unique-device counts.'),
    ('bsmi', 'bsmi_certificates', 'bsmi-certificates-166852', 'BSMI published XML row',
     ['vendor_code', 'certificate_number', 'applicant', 'product_name', 'primary_model'],
     'source_id,row_number',
     'source_id,row_number,vendor_code,certificate_number,applicant,expiry_raw,product_name,primary_model,status_raw',
     'BSMI applicant association only; no manufacturer or operator inferred. Primary model annotations, '
     'raw expiry and statuses remain source claims. Snapshot completeness and current validity are unproven.'),
    ('bsmi_product_certificates', 'bsmi_product_certificates', 'bsmi-additional-162936', 'BSMI published XML row',
     ['certificate_number', 'applicant', 'product_name', 'primary_model'], 'source_id,row_number',
     'source_id,row_number,certificate_number,applicant,expiry_raw,contact_name,contact_phone,product_name,primary_model,status_raw',
     'Applicant only, not manufacturer or operator. Raw dates, model annotations and blanks retained; '
     'no current validity or complete historical coverage inferred.'),
    ('bsmi_voluntary_certificates', 'bsmi_voluntary_certificates', 'bsmi-additional-30176', 'BSMI published XML row',
     ['certificate_number', 'applicant', 'product_name', 'primary_model'], 'source_id,row_number',
     'source_id,row_number,certificate_number,applicant,expiry_raw,product_name,primary_model',
     'Applicant only, not manufacturer or operator. Raw dates, model annotations and blanks retained; '
     'no current validity or complete historical coverage inferred.'),
    ('bsmi_declarations', 'bsmi_declarations', 'bsmi-additional-169189', 'BSMI published XML row',
     ['registration_number', 'company_name', 'product_name', 'primary_model', 'business_number'], 'source_id,row_number',
     'source_id,row_number,registration_number,company_name,business_number,product_name,company_address,contact_phone,primary_model,application_date_raw',
     'Registered company only, not manufacturer or operator. Raw dates, model annotations and blanks retained; '
     'no current validity or complete historical coverage inferred.'),
    ('ncc', 'ncc_equipment', 'ncc-equipment-5996', 'NCC published CSV row',
     ['equipment_type', 'certificate_number', 'trade_certificate_number', 'equipment_name', 'brand', 'model',
      'applicant', 'manufacturer', 'certification_body', 'charger', 'battery', 'test_lab', 'test_lab_country'],
     'source_id,row_number',
     'source_id,row_number,certificate_number,trade_certificate_number,equipment_name,brand,model,applicant,manufacturer,approval_date_raw,certificate_validity_raw,revocation_date_raw',
     'NCC Taiwan source claims: applicant, manufacturer and brand are separate. Confidentiality labels and blanks '
     'are preserved. Validity flags are snapshot values, not a current determination; no operator attribution.'),
    ('tec', 'tec_equipment', 'tec-register', 'historical HTML table row',
     ['applicant', 'declared_oem', 'primary_model', 'additional_models', 'certificate_id', 'product_name', 'product_variant'],
     'source_id,row_number',
     'source_id,row_number,applicant,declared_oem,primary_model,additional_models,product_name,product_variant,certificate_id,certificate_category,certificate_type,issue_date_raw,valid_upto_raw,issue_date,valid_upto,parse_status,issue,cells_json,source_url',
     'Historical table; applicant and declared OEM are distinct. Additional models are not aliases. '
     'Blank OEMs remain unknown; repeated certificates remain separate rows. Invalid dates require review. '
     'No present approval validity, support deadline, factory or operator inference.'),
    ('anatel', 'anatel_equipment', 'anatel-products', 'published register row',
     ['model', 'commercial_name', 'manufacturer', 'applicant', 'approval_number', 'certificate_number'],
     'manufacturer,model,row_number',
     'source_id,row_number,model,commercial_name,manufacturer,applicant,approval_number,certificate_number,certificate_status,application_status',
     'Source manufacturer/applicant claims; commercial names and variants are not inferred aliases.'),
    ('icasa', 'equipment_register', 'icasa-2025', 'PDF table row',
     ['name', 'model', 'category', 'approval_number', 'cells_json'],
     'source_id,page,row_number',
     'source_id,page,row_number,name,model,category,approval_number,parse_status,cells_json,source_url',
     'No manufacturer column. review_required rows retain raw cells and have no asserted model.'),
    ('bahrain', 'bahrain_equipment', 'bahrain-tra', 'published register row',
     ['model_number', 'manufacturer', 'marketing_name', 'device_description', 'authority_ref', 'device_remark', 'authority_remark'],
     'manufacturer,model_number,authority_ref',
     'source_id,authority_ref,manufacturer,model_number,marketing_name,device_description,device_remark,authority_remark,expiry_date,status_code',
     'Regulatory manufacturer claims; no inferred factory, operator or current approval validity.'),
    ('fcc_grantees', 'fcc_grantees', 'fcc-grantees', 'historical company registration',
     ['grantee_name', 'grantee_code'], 'grantee_name,grantee_code',
     'source_id,grantee_code,grantee_name,country,date_received',
     'Historical company registrations, not device authorizations; consult the source snapshot date.'),
]


def contains(value, query):
    return query.casefold() in (value or '').casefold()


def text_excerpt(value, query):
    position = value.casefold().find(query.casefold())
    # Map expanded case folds (e.g. sharp-s) back to original character offsets.
    offsets = [i for i, ch in enumerate(value) for _ in ch.casefold()]
    start = max(0, offsets[position] - 100) if position >= 0 else 0
    end = min(len(value), start + 500)
    return ('… ' if start else '') + value[start:end] + (' …' if end < len(value) else '')


def cells_text(value):
    cells = json.loads(value)
    if not isinstance(cells, list) or any(v is not None and not isinstance(v, str) for v in cells):
        raise ValueError('Invalid cached registry cells')
    return '\n'.join(v or '' for v in cells)


def source_metadata(db, ids):
    if not ids:
        return []
    return [dict(r) for r in db.execute(
        'SELECT id,url,tier,coverage,records_seen,upstream_revision,fetched_at FROM sources '
        'WHERE id IN (' + ','.join('?' for _ in ids) + ') ORDER BY id', ids)]


def section(key, unit, stored, total, rows, sources, scope):
    return dict(dataset=key, unit=unit, state='loaded' if sources else 'not_loaded',
                stored_rows=stored, matches=total, shown=len(rows), truncated=total > len(rows),
                scope=scope, sources=sources, rows=rows)


def matching_rows(db, table, columns, clause, args, order, limit):
    """Count and select with one filtered scan; SQL fragments are internal constants."""
    filtered = ' FROM ' + table + ' WHERE ' + clause
    if limit == 0:
        return db.execute('SELECT count(*)' + filtered, args).fetchone()[0], []
    if limit == -1:
        rows = [dict(r) for r in db.execute('SELECT ' + columns + filtered + ' ORDER BY ' + order, args)]
        return len(rows), rows
    # The window includes all matches before LIMIT; only the requested rows
    # cross into Python. Remove the private count before exposing any results.
    rows = [dict(r) for r in db.execute(
        'SELECT ' + columns + ',count(*) OVER () AS _lookup_match_count' + filtered +
        ' ORDER BY ' + order + ' LIMIT ?', args + (limit,))]
    total = rows[0]['_lookup_match_count'] if rows else 0
    for row in rows:
        del row['_lookup_match_count']
    return total, rows


def run(db, query, limit=20):
    if not isinstance(query, str) or not query.strip():
        raise ValueError('Lookup requires a nonempty query')
    if type(limit) is not int or limit < -1:
        raise ValueError('Limit must be -1 or nonnegative')
    query = query.strip()
    db.create_function('dh_contains', 2, contains, deterministic=True)
    db.create_function('dh_cells', 1, cells_text, deterministic=True)
    # Keep counts, rows and source metadata in the same snapshot, without
    # committing or rolling back an existing caller-owned transaction.
    own_transaction = not db.in_transaction
    if own_transaction:
        db.execute('BEGIN')
    try:
        return _run(db, query, limit)
    finally:
        if own_transaction:
            db.rollback()


def _run(db, query, limit):
    sections = []
    clause = ('dh_contains(d.title || char(10) || d.brand || char(10) || d.model || char(10) || d.revision, ?) '
              # FTS device_id is unindexed. Collect matching IDs once instead
              # of rescanning the virtual table for every catalog identity.
              'OR d.id IN (SELECT device_id FROM search_index WHERE dh_contains(text, ?))')
    args = (query, query)
    total, rows = matching_rows(db, 'devices d', 'd.*', clause, args,
                                'd.brand,d.model,d.revision,d.id', limit)
    for row in rows:
        row['identity_text_matches'] = []
        if row['model'].casefold() == query.casefold():
            row['identity_text_matches'].append({'kind': 'model', 'value': row['model']})
        row['evidence'] = [dict(r) for r in db.execute(
            'SELECT DISTINCT r.source_id,r.source_url,s.tier,a.company,a.role '
            'FROM records r JOIN sources s ON s.id=r.source_id '
            'LEFT JOIN affiliations a ON a.record_id=r.id WHERE r.device_id=? '
            'ORDER BY r.source_id,r.source_url,a.company,a.role', (row['id'],))]
        for item in db.execute('SELECT DISTINCT i.kind,i.value FROM identifiers i JOIN records r '
                               'ON r.id=i.record_id WHERE r.device_id=? ORDER BY i.kind,i.value', (row['id'],)):
            if item['kind'] in IDENTITY_KINDS and item['value'].casefold() == query.casefold():
                row['identity_text_matches'].append(dict(item))
    ids = [r[0] for r in db.execute('SELECT DISTINCT source_id FROM records ORDER BY source_id')]
    sections.append(section('catalog', 'catalog identity', db.execute('SELECT count(*) FROM devices').fetchone()[0],
                            total, rows, source_metadata(db, ids),
                            'Literal match against catalog text, including facts and exhibit titles. '
                            'Identity text matches are string equality, not corroboration. '
                            'Evidence lists all sources for the identity; use show for per-source details.'))
    for key, table, default_source, unit, fields, order, columns, scope in REGISTRIES:
        if not db.execute("SELECT 1 FROM sqlite_master WHERE type IN ('table','view') AND name=?", (table,)).fetchone():
            sections.append(section(key, unit, 0, 0, [], [], scope + ' No table in this database; import this source to add it.'))
            continue
        expressions = ['dh_cells(cells_json)' if f == 'cells_json' else "coalesce(" + f + ",'')" for f in fields]
        clause = 'dh_contains(' + ' || char(10) || '.join(expressions) + ', ?)'
        totals = list(db.execute('SELECT source_id,count(*) FROM ' + table + ' GROUP BY source_id'))
        stored = sum(r[1] for r in totals)
        ids = sorted({r[0] for r in totals} | (set(default_source) if isinstance(default_source, list) else {default_source}))
        if key == 'rra':
            # A verified zero query has source metadata but intentionally no
            # equipment rows. Keep it visible instead of reporting not_loaded.
            ids = sorted(set(ids) | {r[0] for r in db.execute(
                "SELECT id FROM sources WHERE id LIKE 'rra-official-query-%'")})
        if key == 'ised':
            ids = sorted(set(ids) | {r[0] for r in db.execute(
                "SELECT id FROM sources WHERE id LIKE 'ised-rel-query-%'")})
        if key == 'japan':
            # Supplemental queries can return zero rows; retain their scope too.
            ids = sorted(set(ids) | {r[0] for r in db.execute(
                "SELECT id FROM sources WHERE id LIKE 'japan-mic-mirror-%'")})
        sources = source_metadata(db, ids)
        total, rows = matching_rows(db, table, columns, clause, (query,), order, limit)
        urls = {s['id']: s['url'] for s in sources}
        for row in rows:
            row.setdefault('source_url', urls.get(row['source_id']))
            if key == 'fcc_excerpts':
                row['excerpt'] = text_excerpt(row.pop('text'), query)
            if key == 'fcc_details':
                row['fields'] = json.loads(row.pop('fields_json'))
            if 'cells_json' in row:
                row['raw_cells'] = json.loads(row.pop('cells_json'))
        sections.append(section(key, unit, stored, total, rows, sources, scope))
    return {'query': query, 'limit_per_dataset': limit,
            'matching': 'Unicode case-insensitive literal substring; whitespace trimmed, punctuation and suffixes preserved.',
            'scope': 'Search of this local snapshot only. Counts use different units and must not be summed as unique devices. '
                     'A match is a discovery lead, not an inventory-validation pass. ISED labelled target records and UL assertions are in catalog.',
            'datasets': sections}
