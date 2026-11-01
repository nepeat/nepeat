"""Anatel's complete published CSV snapshot, with scoped manufacturer assertions."""
import csv
import html
import io
import json
from pathlib import Path
from zipfile import BadZipFile, ZipFile

from .catalog import digest, target_for

SOURCE = 'anatel-products'
URL = 'https://www.anatel.gov.br/dadosabertos/paineis_de_dados/certificacao_de_produtos/produtos_certificados.zip'
PAGE = 'https://informacoes.anatel.gov.br/paineis/certificacao-de-produtos/consulta-de-produtos'
MEMBER = 'Produtos_Homologados_Anatel.csv'
FIELDS = [
    ('approval_date', 'Data da Homologação'), ('approval_number', 'Número de Homologação'),
    ('applicant', 'Nome do Solicitante'), ('applicant_cnpj', 'CNPJ do Solicitante'),
    ('certificate_number', 'Certificado de Conformidade Técnica'),
    ('certificate_date', 'Data do Certificado de Conformidade Técnica'),
    ('certificate_expiry', 'Data de Validade do Certificado'),
    ('certificate_status_code', 'Código de Situação do Certificado'),
    ('certificate_status', 'Situação do Certificado'),
    ('application_status_code', 'Código de Situação do Requerimento'),
    ('application_status', 'Situação do Requerimento'), ('manufacturer', 'Nome do Fabricante'),
    ('model', 'Modelo'), ('commercial_name', 'Nome Comercial'),
    ('product_category', 'Categoria do Produto'), ('product_type', 'Tipo do Produto'),
    ('antenna_flag', 'IC_ANTENA'), ('active_flag', 'IC_ATIVO'),
    ('manufacturer_country', 'País do Fabricante'), ('country_itu', 'CodUIT'), ('country_iso', 'CodISO'),
]


def parse_archive(path):
    try:
        with ZipFile(path) as archive:
            if archive.namelist() != [MEMBER]:
                raise ValueError('Anatel archive members changed')
            member = archive.getinfo(MEMBER)
            if member.file_size > 150_000_000:
                raise ValueError('Anatel CSV exceeds reviewed size bound')
            with io.TextIOWrapper(archive.open(member), encoding='utf-8-sig', newline='') as stream:
                reader = csv.reader(stream, delimiter=';', strict=True)
                if next(reader, None) != [label for _, label in FIELDS]:
                    raise ValueError('Anatel CSV headers changed')
                rows = []
                for row in reader:
                    if len(row) != len(FIELDS):
                        raise ValueError('Malformed Anatel CSV record ' + str(len(rows) + 1))
                    rows.append(tuple(row))
    except (BadZipFile, UnicodeError, csv.Error) as exc:
        raise ValueError('Invalid Anatel CSV archive') from exc
    if not rows:
        raise ValueError('Empty Anatel CSV')
    return rows


def validate_bundle(path):
    path = Path(path)
    bundle = json.loads(path.read_text())
    if bundle.get('schema_version') != 1 or bundle.get('source_url') != URL:
        raise ValueError('Unrecognized Anatel bundle')
    entries = bundle.get('files', {})
    if set(entries) != {'before', 'after', 'index'}:
        raise ValueError('Incomplete Anatel snapshot bundle')
    paths = {}
    for key, entry in entries.items():
        name = entry.get('path')
        if (not isinstance(name, str) or Path(name).name != name
                or entry.get('url') != (PAGE if key == 'index' else URL)):
            raise ValueError('Invalid Anatel snapshot reference')
        file = (path.parent / name).resolve()
        if file.parent != path.parent.resolve():
            raise ValueError('Anatel snapshot escapes bundle directory')
        if digest(file.read_bytes()) != entry.get('sha256'):
            raise ValueError('Anatel snapshot checksum mismatch')
        paths[key] = file
    if entries['before']['sha256'] != entries['after']['sha256']:
        raise ValueError('Anatel archive changed during capture')
    if URL not in html.unescape(paths['index'].read_text()):
        raise ValueError('Anatel source index lacks the archive link')
    rows = parse_archive(paths['before'])
    if type(bundle.get('rows')) is not int or bundle['rows'] != len(rows):
        raise ValueError('Anatel snapshot record count mismatch')
    return rows


def collect(raw, fetch, refresh=False):
    raw = Path(raw)
    path = raw / 'anatel-bundle.json'
    if path.exists() and not refresh:
        return path
    entries = {}
    for key, url, name in [('before', URL, 'anatel-products.zip'),
                           ('index', PAGE, 'anatel-products-index.html'),
                           ('after', URL, 'anatel-products-after.zip')]:
        file = fetch(url, raw / name, True, max_bytes=150_000_000)
        entries[key] = {'url': url, 'path': name, 'sha256': digest(file.read_bytes())}
    count = len(parse_archive(raw / entries['before']['path']))
    tmp = path.with_suffix('.part')
    tmp.write_text(json.dumps({'schema_version': 1, 'source_url': URL, 'rows': count, 'files': entries}, indent=2) + '\n')
    try:
        validate_bundle(tmp)
        tmp.replace(path)
    finally:
        if tmp.exists():
            tmp.unlink()
    return path


def import_bundle(c, path):
    rows = validate_bundle(path)
    c.source(SOURCE, URL, 'regulatory', 'Anatel public factual equipment register; credit Anatel; source rights retained',
             'All records in the official published ZIP/CSV, including duplicates and empty model cells. '
             'Two archive downloads agree byte-for-byte and the official index links the archive; '
             'no independent database total is exposed, so completeness is limited to the published file. '
             'Catalog includes exact target manufacturer matches with nonempty models. Applicant, commercial-name '
             'and raw status/date fields remain distinct. No present approval validity, factory or deployment is inferred.', path)
    c.db.execute('DELETE FROM anatel_equipment WHERE source_id=?', (SOURCE,))
    c.db.executemany('INSERT INTO anatel_equipment VALUES (' + ','.join('?' for _ in range(23)) + ')',
                     ((SOURCE, n, *row) for n, row in enumerate(rows, 1)))
    count = 0
    for n, row in enumerate(rows, 1):
        r = dict(zip((key for key, _ in FIELDS), row))
        target = target_for(r['manufacturer'])
        if target is None or not r['model'].strip():
            continue
        facts = [(k, v) for k, v in r.items() if k not in {'manufacturer', 'model'} and v]
        facts += [('registry_manufacturer', r['manufacturer']), ('csv_record_number', str(n)),
                  ('evidence_scope', 'Registry manufacturer/model assertion; commercial-name cells are unparsed facts, not model aliases. Raw dates and statuses do not imply present validity or unit provenance.')]
        ids = [('anatel_approval_id', r['approval_number'])] if r['approval_number'] else []
        c.add(SOURCE, str(n), PAGE, target, r['model'], target + ' ' + r['model'], r['product_type'], r,
              [(r['manufacturer'], 'registry_manufacturer'), (r['applicant'], 'regulatory_applicant')], ids, facts)
        count += 1
    c.finish(SOURCE, len(rows))
    return {'registry_rows': len(rows), 'model_assertions': count}


def search(db, query='', limit=100):
    if limit < -1:
        raise ValueError('Limit must be nonnegative or -1')
    # instr treats user text literally; wildcard characters cannot broaden the query.
    return [dict(r) for r in db.execute(
        'SELECT row_number,approval_number,manufacturer,model,commercial_name,applicant,certificate_status,application_status '
        'FROM anatel_equipment WHERE instr(lower(manufacturer || char(10) || model || char(10) || '
        'commercial_name || char(10) || applicant || char(10) || approval_number),lower(?))>0 '
        'ORDER BY manufacturer,model,row_number LIMIT ?', (query, limit))]
