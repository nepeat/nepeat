"""Discover Oracle hardware names in software-source and tested-platform pages."""
import html
import re
from pathlib import Path
from urllib.parse import urljoin

from .importers import Page

URLS = {
    'oracle-oss': 'https://www.oracle.com/downloads/opensource/x86-servers-source-code-downloads.html',
    'oracle-x10-license': 'https://docs.oracle.com/en/engineered-systems/private-cloud-appliance/3.0-latest/licensing-3.0-latest/licensing-component-x10-2c-license.html',
    'nist-4506': 'https://csrc.nist.gov/projects/cryptographic-module-validation-program/certificate/4506',
}


def plain(value):
    value = re.sub(r'<(?:br\b[^>]*|/(?:p|div|li))>', ' ', value, flags=re.I)
    return ' '.join(html.unescape(re.sub(r'<[^>]*>', '', value)).split())


def oss_entries(body):
    if 'Source Code for Open Source Software - X86 Hardware Systems' not in body:
        raise ValueError('Missing Oracle x86 source-index title')
    entries = {}
    for section in re.findall(r'<section\b[^>]*>(.*?)</section>', body, re.S | re.I):
        headings = re.findall(r'<h5\b[^>]*>(.*?)</h5>', section, re.S | re.I)
        if not headings:
            continue
        title = plain(headings[0])
        if not re.match(r'^(Oracle (?:Server|Exadata) |Sun (?:Server|Fire|Blade) )', title):
            continue
        base = title.split(' (formerly ')[0]
        base = re.sub(r' SW \d+\.x$', '', base)
        entry = entries.setdefault(base, {'title': title, 'links': set()})
        for label, href in Page(section).links:
            link = urljoin(URLS['oracle-oss'], href)
            if link.startswith('https://download.oracle.com/'):
                entry['links'].add((label, link))
    if not entries:
        raise ValueError('Oracle source index has no hardware headings')
    return entries


def import_page(c, path, sid):
    body = Path(path).read_text()
    url = URLS[sid]
    records = []
    if sid == 'oracle-oss':
        for model, entry in oss_entries(body).items():
            query = re.sub(r'^(?:Oracle Server|Sun Server|Sun Fire|Sun Blade) ', '', model)
            query = re.sub(r' Server(?: Module)?$', '', query)
            facts = [('source_heading', entry['title']),
                     ('software_evidence', 'Public source-code archive links only; not installable firmware or a current support guarantee')]
            for label, link in sorted(entry['links']):
                facts.extend([('source_archive_label', label), ('source_archive_url', link)])
            records.append((model, [('model_search', query)], facts,
                            {'heading': entry['title'], 'archives': sorted(entry['links'])}))
        scope = 'Hardware headings and archive links in the same section of one cached Oracle source-code index. Some hardware headings have no same-section links; version-only sections are excluded rather than assigning their links to a guessed parent. Software-only headings excluded; suffixes preserved. Archives not downloaded or executed.'
    elif sid == 'oracle-x10-license':
        headings = [plain(h) for h in re.findall(r'<h2\b[^>]*>(.*?)</h2>', body, re.S | re.I)]
        title = 'Oracle Server X10-2c Licensing Information'
        paragraphs = [plain(p) for p in re.findall(r'<p\b[^>]*>(.*?)</p>', body, re.S | re.I)]
        statement = 'The Oracle Server X10-2c is a hardware component that can be included in Oracle Private Cloud Appliance.'
        if title not in headings or statement not in paragraphs:
            raise ValueError('Missing explicit Oracle X10-2c product statement')
        records.append(('Oracle Server X10-2c', [('model_search', 'X10-2c')],
                        [('product_integration', 'May be included in Oracle Private Cloud Appliance')],
                        {'heading': title, 'statement': statement}))
        scope = 'Explicit product statement in cached Oracle licensing documentation. No physical OEM or previous unit ownership assertion.'
    else:
        match = re.search(r'Tested Configuration\(s\)</span>.*?<ul\b[^>]*>(.*?)</ul>', body, re.S)
        if 'Certificate #4506' not in body or not match:
            raise ValueError('Missing NIST certificate 4506 tested configurations')
        models = {}
        for item in re.findall(r'<li\b[^>]*>(.*?)</li>', match[1], re.S | re.I):
            text = plain(item)
            model = re.search(r' running on Oracle ([AEX]\d+(?:-\d+[A-Z]?)?) [Ss]erver\b', text)
            if model:
                models.setdefault(model.group(1), []).append(text)
        if not models:
            raise ValueError('No explicit Oracle server names in NIST tested configurations')
        for model, configurations in models.items():
            records.append(('Oracle Server ' + model, [('model_search', model)],
                            [('cmvp_certificate', '4506'),
                             ('evidence_scope', 'Server used in a tested software-module configuration; not hardware certification, firmware support, or prior ownership')],
                            {'tested_configurations': configurations}))
        scope = 'Oracle server names in the Tested Configurations list of NIST CMVP certificate 4506. The certificate concerns a software module, not certification of the server hardware.'
    c.source(sid, url, 'regulatory' if sid == 'nist-4506' else 'vendor',
             'Extracted factual hardware identifiers and source references; source rights retained', scope, path)
    for model, ids, facts, raw in records:
        c.add(sid, model, url, 'Oracle', model, model, 'server platform', raw,
              [('Oracle', 'tested_platform_vendor' if sid == 'nist-4506' else 'documentation_vendor')], ids, facts)
    c.finish(sid, len(records))
    return len(records)
