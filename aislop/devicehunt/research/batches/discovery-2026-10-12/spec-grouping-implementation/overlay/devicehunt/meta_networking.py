"""Reviewed first-party hardware and platform-use assertions from Meta articles."""
import datetime as dt
import json
from html.parser import HTMLParser
from pathlib import Path

from .catalog import digest
from .specifications import VOID

URLS = {
    'meta-minipack-2019': 'https://engineering.fb.com/2019/03/14/data-center-engineering/f16-minipack/',
    'meta-ocp-2021': 'https://engineering.fb.com/2021/11/09/data-center-engineering/ocp-summit-2021/',
    'meta-ocp-2024': 'https://engineering.fb.com/2024/10/15/data-infrastructure/open-future-networking-hardware-ai-ocp-2024-meta/',
    'meta-ocp-2025': 'https://engineering.fb.com/2025/10/13/data-infrastructure/ocp-summit-2025-the-open-future-of-networking-hardware-for-ai/',
}
DATES = {'meta-minipack-2019': '2019-03-14', 'meta-ocp-2021': '2021-11-09',
         'meta-ocp-2024': '2024-10-15', 'meta-ocp-2025': '2025-10-13'}


class Article(HTMLParser):
    def __init__(self, body):
        super().__init__(convert_charrefs=True)
        self.metadata, self.dates, self.blocks = {}, [], []
        self.stack, self.parts = [], []
        self.block_depth, self.block_tag, self.sections = None, None, 0
        self.feed(body)
        if self.stack or self.sections != 1:
            raise ValueError('Missing or unclosed Meta article content')

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag == 'meta' and a.get('property') in ('og:url', 'article:published_time'):
            self.metadata.setdefault(a['property'], []).append(a.get('content'))
        if tag == 'time' and {'published', 'updated'} <= set(a.get('class', '').split()):
            self.dates.append(a.get('datetime'))
        if 'entry-content' in a.get('class', '').split():
            self.sections += 1
            if self.stack or tag in VOID:
                raise ValueError('Nested Meta article content')
            self.stack = [tag]
        elif self.stack and tag not in VOID:
            self.stack.append(tag)
        if self.stack:
            if tag in ('p', 'figcaption', 'h2', 'h3') and self.block_depth is None:
                self.block_depth, self.block_tag, self.parts = len(self.stack), tag, []
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
                self.blocks.append((self.block_tag, text))
            self.block_depth, self.block_tag, self.parts = None, None, []


def reviewed_records(body, sid):
    article = Article(body)
    if article.metadata.get('og:url') != [URLS[sid]] or article.dates != [DATES[sid]]:
        raise ValueError('Meta article URL or displayed publication date differs')
    timestamps = article.metadata.get('article:published_time', [])
    if len(timestamps) != 1:
        raise ValueError('Missing Meta publication timestamp')
    published = dt.datetime.fromisoformat(timestamps[0])
    if published.tzinfo is None or abs((published.date() - dt.date.fromisoformat(DATES[sid])).days) > 1:
        raise ValueError('Meta publication timestamp conflicts with the displayed date')

    def require(tag, *phrases):
        matches = [(n, text) for n, (kind, text) in enumerate(article.blocks)
                   if kind == tag and all(p in text for p in phrases)]
        if len(matches) != 1:
            raise ValueError('Missing or ambiguous reviewed Meta evidence: ' + repr(phrases))
        n, text = matches[0]
        if any(p in text.lower() for p in ('not manufactured', 'never manufactured', 'no longer manufactured',
                                           'not been deployed', 'never been deployed',
                                           'not served', 'never served', 'no longer served')):
            raise ValueError('Negated Meta evidence requires review')
        return {'block_index': n, 'tag': tag, 'text_sha256': digest(text.encode())}

    def record(brand, model, role, relationship, detail, evidence, facts=(), identifiers=()):
        return dict(brand=brand, model=model, role=role, relationship=relationship, detail=detail,
                    evidence=evidence, facts=list(facts), identifiers=list(identifiers),
                    published_date=DATES[sid], published_timestamp=timestamps[0])

    if sid == 'meta-minipack-2019':
        design = require('p', 'We worked with Edgecore to build Minipack based on our designs.', '128x100GE')
        modules = require('p', 'eight port interface module', 'PIM-16Q', '8x200G QSFP56', 'remaining eight ports', 'PIM-4DD', '4x400G QSFP-DD')
        software = require('p', 'we use FBOSS on Minipack', 'Cumulus Networks', 'SAI/SONIC')
        return [record('Edge-Core', 'Minipack', 'build_partner', 'documented_design_lineage',
                       'Facebook identified Edgecore as its build partner for the Facebook-designed Minipack in 2019; individual unit history is unknown.',
                       [design, modules, software], [
                           ('ports', '128 x 100GE platform; configuration depends on installed PIM modules'),
                           ('physical_description', 'Modular chassis with eight port-interface-module positions'),
                           ('module_configuration', 'PIM-16Q: 16 x 100G QSFP28 or 8 x 200G QSFP56; eight ports are inactive in 200G mode'),
                           ('module_configuration', 'PIM-4DD: 4 x 400G QSFP-DD; article permits mixed PIM-16Q/PIM-4DD configurations'),
                           ('software_installation_description', '2019 article reports Facebook using FBOSS and external Cumulus/SAI-SONiC ports; no current download, licence or installation guarantee')])]
    if sid == 'meta-ocp-2021':
        manufacturing = require('p', 'Both the Wedge400 and 400C are manufactured by Celestica',
                                'already been deployed in our data centers', '12.8 Tbps', 'field-replaceable CPU')
        chips = require('p', 'Wedge 400 utilizes Broadcom', 'Tomahawk 3', '400C uses Cisco', 'Silicon One')
        return [record('Celestica', model, 'manufacturer', 'documented_platform_deployment',
                       'Meta reported this Celestica-manufactured platform already deployed in its data centers on 2021-11-09; this does not establish provenance of a surplus unit.',
                       [manufacturing, chips], [('asic', chip), ('switching_capacity', '12.8 Tbps'),
                                               ('physical_description', 'Field-replaceable CPU subsystem'),
                                               ('source_model_group', 'Wedge 400/400C; the shared Wedge prefix is expanded for the 400C entry')], ids)
                for model, chip, ids in [('Wedge 400', 'Broadcom Tomahawk 3', [('model_search', 'Wedge400')]),
                                          ('Wedge 400C', 'Cisco Silicon One', [])]]
    if sid == 'meta-ocp-2024':
        manufacturing = require('figcaption', 'Minipack3 (Broadcom Tomahawk5', 'designed by Meta and manufactured by Celestica', '51.2T')
        planned = require('p', 'Meta will deploy', 'Minipack3')
        ports = require('p', 'Minipack3 utilizes', 'Tomahawk5', '51.2 Tbps', '64x OSFP')
        software = require('p', 'Meta will run', 'Minipack3', 'FBOSS')
        return [record('Celestica', 'Minipack3', 'manufacturer', 'announced_platform_deployment',
                       'Meta identified its Minipack3 design as manufactured by Celestica and announced intended deployment in October 2024; the announcement is not proof of a specific unit deployment.',
                       [manufacturing, planned, ports, software], [('asic', 'Broadcom Tomahawk5'),
                       ('switching_capacity', '51.2 Tbps'), ('ports', '64 x OSFP'),
                       ('software_installation_description', '2024 announcement planned FBOSS operation; no installable image or support entitlement asserted')])]
    manufacturing = require('figcaption', 'Minipack3N', 'designed by Meta and manufactured by Accton', '51.2 Tbps', 'NVIDIA Spectrum-4')
    introduction = require('p', 'introducing Minipack3N', 'NVIDIA Spectrum-4', 'same system design as Minipack3')
    fabric_role = require('p', 'Minipack3 (based on Broadcom Tomahawk5)', 'Cisco 8501',
                         'These platforms have served as the foundation for building our next-generation frontend and backend data center fabrics.')
    return [record('Accton', 'Minipack3N', 'manufacturer', 'documented_design_lineage',
                   'Meta introduced Minipack3N in its October 2025 publication, explicitly naming Accton as manufacturer and Meta as designer; no specific unit deployment is asserted.',
                   [manufacturing, introduction], [('asic', 'NVIDIA Spectrum-4'), ('switching_capacity', '51.2 Tbps')]),
            record('Celestica', 'Minipack3', None, 'documented_fabric_role',
                   'Meta reported in October 2025 that Minipack3 and Cisco 8501 had provided a foundation for building its next-generation frontend and backend data-center fabrics. This is a reported platform role, not an explicit completed-deployment statement, current-use guarantee or auction-unit provenance. The Celestica identity namespace comes from the separately validated 2024 article; this 2025 record makes no manufacturer assertion.',
                   [fabric_role])]


def import_bundle(c, path):
    path = Path(path)
    bundle = json.loads(path.read_text())
    if bundle.get('schema_version') != 1 or set(bundle.get('articles', {})) != set(URLS):
        raise ValueError('Incomplete Meta networking bundle')
    prepared = []
    for sid, url in URLS.items():
        entry = bundle['articles'][sid]
        name = entry.get('path')
        if not isinstance(name, str) or Path(name).name != name or entry.get('url') != url:
            raise ValueError('Invalid Meta article dependency')
        file = (path.parent / name).resolve()
        if file.parent != path.parent.resolve():
            raise ValueError('Meta article escapes bundle directory')
        body = file.read_bytes()
        if digest(body) != entry.get('sha256'):
            raise ValueError('Meta article checksum mismatch')
        prepared.append((sid, file, reviewed_records(body.decode('utf-8'), sid)))
    # All dependencies and claims have passed before any source is replaced.
    count = 0
    for sid, file, records in prepared:
        c.source(sid, URLS[sid], 'vendor', 'Extracted factual hardware assertions; Meta source rights retained',
                 'Reviewed first-party platform-designer/operator article dated ' + DATES[sid] + '. '
                 'Named target-company manufacturing or build-partner claims are kept separate from design, '
                 'historical platform deployment and announced plans. No retail availability, firmware licence, '
                 'complete product inventory, cross-brand equivalence or individual-unit provenance assertion. '
                 'Both displayed publication date and metadata timestamp are retained.', file)
        for r in records:
            facts = r['facts'] + [('published_date', r['published_date']), ('published_timestamp', r['published_timestamp']),
                                  ('evidence_scope', 'Historical platform-level assertions; not the configuration or previous owner of a particular unit')]
            affiliations = [(r['brand'], r['role']), ('Meta', 'platform_designer')]
            if r['role'] is None:
                # The 2025 paragraph names the model but does not name its manufacturer.
                # All four articles, including the 2024 manufacturer caption, were
                # validated above; it supplies only the existing identity namespace.
                affiliations = [('Meta', 'documentation_vendor')]
                facts += [('identity_namespace_basis_source', 'meta-ocp-2024'),
                          ('identity_namespace_basis_url', URLS['meta-ocp-2024']),
                          ('identity_namespace_basis_sha256', bundle['articles']['meta-ocp-2024']['sha256']),
                          ('identity_namespace_scope', 'Same explicitly named Minipack3 platform; namespace from the 2024 manufacturer caption, not a new 2025 Celestica manufacturing claim')]
            c.add(sid, r['model'], URLS[sid], r['brand'], r['model'], r['model'], 'network switch', r,
                  affiliations, r['identifiers'], facts,
                  [('Facebook / Meta', r['relationship'], r['detail'])])
            count += 1
        c.finish(sid, len(records))
    return {'sources': len(prepared), 'model_assertions': count}
