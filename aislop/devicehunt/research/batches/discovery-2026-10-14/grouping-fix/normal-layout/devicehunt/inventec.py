"""Import public Inventec server-index cards; never fetch gated product details."""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import quote, urljoin, urlsplit

URL = 'https://ebg.inventec.com/en/product'
SOURCE = 'inventec-server-index-en'


class Cards(HTMLParser):
    def __init__(self, text):
        super().__init__(convert_charrefs=True)
        self.rows = []
        self.card = None
        self.heading = False
        self.subtitle_depth = 0
        self.feed(text)
        self.close()
        if self.card is not None:
            raise ValueError('Unclosed Inventec product card')

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == 'a':
            if self.card is not None:
                raise ValueError('Nested Inventec product link')
            href = attrs.get('href', '')
            parsed = urlsplit(urljoin(URL, href))
            if (parsed.scheme == 'https' and parsed.netloc == 'ebg.inventec.com'
                    and parsed.path.startswith('/en/product/Server/')
                    and not parsed.query and not parsed.fragment):
                self.card = dict(href_as_printed=href, model_parts=[], subtitle_parts=[],
                                 detail_login_required=False)
        if self.card is None:
            return
        classes = attrs.get('class', '').split()
        if tag == 'h3':
            if self.card['model_parts'] or self.heading:
                raise ValueError('Multiple headings in Inventec product card')
            self.heading = True
        if tag == 'div':
            if self.subtitle_depth:
                self.subtitle_depth += 1
            elif 'p-title' in classes:
                self.subtitle_depth = 1
        if 'member-mask' in classes:
            self.card['detail_login_required'] = True
        if tag == 'br':
            self.handle_data(' ')

    def handle_data(self, data):
        if self.card is not None:
            if self.heading:
                self.card['model_parts'].append(data)
            if self.subtitle_depth:
                self.card['subtitle_parts'].append(data)

    def handle_endtag(self, tag):
        if self.card is None:
            return
        if tag == 'h3':
            self.heading = False
        if tag == 'div' and self.subtitle_depth:
            self.subtitle_depth -= 1
        if tag == 'a':
            # Repeated View more links are navigation, not model assertions.
            if self.card['model_parts']:
                model = ' '.join(''.join(self.card.pop('model_parts')).split())
                subtitle = ' '.join(''.join(self.card.pop('subtitle_parts')).split())
                if not model or len(model) > 300 or self.heading or self.subtitle_depth:
                    raise ValueError('Invalid Inventec product card')
                self.rows.append(dict(self.card, model=model, subtitle=subtitle))
            self.card = None
            self.heading = False
            self.subtitle_depth = 0


def parse(text):
    rows = Cards(text).rows
    if not rows:
        raise ValueError('No Inventec server model cards found')
    seen_models, seen_urls = set(), set()
    for row in rows:
        row['product_url'] = quote(urljoin(URL, row['href_as_printed']), safe=':/()%-._~')
        if row['model'] in seen_models or row['product_url'] in seen_urls:
            raise ValueError('Duplicate or conflicting Inventec server card')
        seen_models.add(row['model'])
        seen_urls.add(row['product_url'])
    return rows


def import_page(c, path):
    path = Path(path).resolve()
    rows = parse(path.read_text())  # Validate the full capture before replacement.
    with c.db:
        c.source(SOURCE, URL, 'vendor', 'Inventec public model facts; upstream rights retained',
            'Public English server-index cards only, including publicly displayed names of '
            'login-restricted products. No linked page, document, image, authentication or '
            'network request. Exact headings and suffixes retained. Public index visibility '
            'is not complete historical coverage, manufacture, operator deployment, release '
            'date, firmware access or an installed auction-unit configuration.', path)
        for row in rows:
            facts = [('product_url', row['product_url']),
                     ('index_url', URL),
                     ('detail_access_status', 'login required; not fetched' if row['detail_login_required']
                      else 'not fetched; public index evidence only')]
            # Restricted cards contribute only their public name and access status.
            if row['subtitle'] and not row['detail_login_required']:
                facts.append(('public_index_subtitle', row['subtitle']))
            c.add(SOURCE, row['product_url'], URL, 'Inventec', row['model'],
                  'Inventec ' + row['model'], 'server platform', row,
                  [('Inventec', 'brand'), ('Inventec', 'platform_vendor')], facts=facts)
        c.finish(SOURCE, len(rows))
    return {'model_assertions': len(rows),
            'public_names_with_gated_details': sum(row['detail_login_required'] for row in rows)}
