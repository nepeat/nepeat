"""Celestica networking product cards from a hashed original HTML snapshot."""
import json
import re
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urljoin, urlsplit

from .documents import dependency
from .importers import Page
from .oracle_evidence import plain

URL = 'https://www.celestica.com/our-expertise/platform-solutions/networking'
SOURCE = 'celestica-networking-index'
LEGACY_SOURCE = 'celestica-networking-webreview'
MODEL = r'(?:DS|ES|EG)\d{4}'


class Tiles(HTMLParser):
    def __init__(self, text):
        super().__init__(convert_charrefs=False)
        self.tiles = []; self.depth = 0; self.parts = []
        self.feed(text); self.close()
        if self.depth:
            raise ValueError('Truncated Celestica product tile')

    def handle_starttag(self, tag, attrs):
        classes = dict(attrs).get('class', '').split()
        if tag == 'div' and 'market-tile' in classes:
            if self.depth:
                raise ValueError('Nested Celestica product tile')
            self.depth = 1; self.parts = [self.get_starttag_text()]; return
        if self.depth:
            if tag == 'div': self.depth += 1
            self.parts.append(self.get_starttag_text())

    def handle_startendtag(self, tag, attrs):
        if self.depth: self.parts.append(self.get_starttag_text())

    def handle_endtag(self, tag):
        if self.depth:
            self.parts.append('</' + tag + '>')
            if tag == 'div':
                self.depth -= 1
                if self.depth:
                    return
                self.tiles.append(''.join(self.parts)); self.parts = []

    def handle_data(self, text):
        if self.depth: self.parts.append(text)

    def handle_entityref(self, name):
        if self.depth: self.parts.append('&' + name + ';')

    def handle_charref(self, name):
        if self.depth: self.parts.append('&#' + name + ';')


def product_cards(text):
    if 'Networking Switches | Celestica' not in plain(text):
        raise ValueError('Celestica networking page title changed')
    cards = []; ignored = []; seen = set()
    for tile in Tiles(text).tiles:
        headings = re.findall(r'<h3\b[^>]*>(.*?)</h3>', tile, re.S | re.I)
        if len(headings) != 1:
            raise ValueError('Celestica tile heading structure changed')
        links = Page(headings[0]).links
        if len(links) != 1:
            raise ValueError('Celestica tile requires one linked heading')
        heading, href = links[0]
        model_text = heading.removesuffix(' - Newest Addition')
        if not re.fullmatch(MODEL + r'(?:/' + MODEL + r')*', model_text):
            # Explicitly keep excluded related-resource headings in the reviewed bundle.
            if re.search(MODEL, heading):
                raise ValueError('Unrecognized model heading qualifier')
            ignored.append(heading); continue
        models = model_text.split('/')
        if len(set(models)) != len(models) or seen.intersection(models):
            raise ValueError('Duplicate Celestica model card')
        link = urljoin(URL, href); parsed = urlsplit(link)
        if (parsed.scheme != 'https' or parsed.hostname not in ('www.celestica.com', 'cls.celestica.com')
                or parsed.username or parsed.password or parsed.port or parsed.query or parsed.fragment):
            raise ValueError('Unexpected Celestica product-link destination')
        after = re.split(r'</h3>', tile, maxsplit=1, flags=re.I)[1]
        # The page has nested <p> tags. Read only the introductory description,
        # ending before the first Optimized-for paragraph, not the whole tile.
        paragraphs = re.split(r'<p\b[^>]*>', after, flags=re.I)
        descriptions = []
        for paragraph in paragraphs[1:]:
            description = plain(re.split(r'</p>', paragraph, maxsplit=1, flags=re.I)[0])
            if not description: continue
            if description.startswith('Optimized for:'): break
            descriptions.append(description)
        if len(descriptions) != 1 or not all(model in descriptions[0] for model in models):
            raise ValueError('Missing or ambiguous model-specific description')
        cards.append(dict(heading=heading, models=models, description=descriptions[0], product_url=link))
        seen.update(models)
    if not cards:
        raise ValueError('No Celestica product cards')
    return dict(cards=cards, ignored_headings=ignored)


def import_bundle(c, path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if data.get('schema_version') != 1 or data.get('url') != URL:
        raise ValueError('Celestica networking bundle scope changed')
    parsed = product_cards(dependency(path.parent, data['html']).decode('utf-8-sig'))
    if parsed != data.get('expected'):
        raise ValueError('Celestica networking cards differ from reviewed snapshot')
    old = c.db.execute('SELECT url FROM sources WHERE id=?', (LEGACY_SOURCE,)).fetchone()
    if old and old[0] != URL:
        raise ValueError('Legacy Celestica source URL differs')
    # Ownership of this source moves out of curated.json only after complete validation.
    c.db.execute('DELETE FROM records WHERE source_id=?', (LEGACY_SOURCE,))
    c.db.execute('DELETE FROM sources WHERE id=?', (LEGACY_SOURCE,))
    c.source(SOURCE, URL, 'vendor', 'Celestica original HTML; factual model and product descriptions, source rights retained',
             'All reviewed networking model cards in one original HTML snapshot. Related-resource '
             'tiles excluded explicitly. Grouped headings expand into separate model identities '
             'sharing qualified descriptions, not aliases. Brand association only; no physical '
             'manufacturer, factory, operator, current availability, firmware entitlement or '
             'complete historical portfolio inferred.', path)
    count = 0
    for card in parsed['cards']:
        for model in card['models']:
            description = card['description']
            facts = [('product_heading', card['heading']), ('product_description', description),
                     ('product_url', card['product_url']), ('index_url', URL)]
            if len(card['models']) > 1:
                facts.append(('description_scope', 'Shared card for ' + card['heading'] +
                              '; alternatives are not allocated to individual models and are not aliases.'))
            factors = re.findall(r'\b\d+\s*(?:OU|U)\b', description)
            if factors:
                facts += [('form_factor', factor) for factor in sorted(set(factors))]
                facts.append(('physical_description', description))
            if re.search(r'\b(?:cooled|cooling)\b', description, re.I):
                facts.append(('cooling_description', description))
            if 'AMD Helios' in description:
                facts.append(('vendor_integration', description))
            c.add(SOURCE, model, URL, 'Celestica', model, 'Celestica ' + model,
                  'network gateway' if model.startswith('EG') else 'switch', card,
                  [('Celestica', 'brand')], facts=facts)
            count += 1
    c.finish(SOURCE, count)
    return dict(product_cards=len(parsed['cards']), model_assertions=count,
                ignored_headings=parsed['ignored_headings'])
