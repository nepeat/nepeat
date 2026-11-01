"""Bounded import of a public seller page, labeled as seller evidence."""
import html
import re
from pathlib import Path

URL = 'https://www.ecrunch.com/edge-core-7760-32x-a-ac-f-as7760-32-100g-ethernet-data-center-spine-switch-red-i610661.html'


def import_ecrunch(c, path):
    body = Path(path).read_text()
    headings = re.findall(r'<h1\b[^>]*>(.*?)</h1>', body, re.S | re.I)
    if len(headings) != 1:
        raise ValueError('Expected one seller product heading')
    title = ' '.join(html.unescape(re.sub(r'<[^>]+>', '', headings[0])).split())
    model = re.match(r'^EDGE-CORE ([A-Z0-9]+(?:-[A-Z0-9]+){2,})\s', title, re.I)
    skus = re.findall(r'itemprop="sku"[^>]*>\s*([^<]+)', body)
    if not model or skus != ['I610661']:
        raise ValueError('Seller page brand/model or expected listing SKU changed')
    value = model.group(1)
    c.source('ecrunch-I610661', URL, 'marketplace', 'Seller factual listing identifiers; seller content rights retained',
             'One cached public seller page. Heading-derived identity and seller SKU only. '
             'Seller-reported brand/model; no verified specifications, availability, price, OEM or AWS provenance.', path)
    c.add('ecrunch-I610661', 'I610661', URL, 'Edge-Core', value, 'Edge-Core ' + value,
          'seller-listed hardware', {'heading': title, 'sku': 'I610661'},
          [('Edge-Core', 'seller_reported_brand')], [('seller_sku', 'I610661')],
          [('evidence_scope', 'Seller heading identifies this ordering code; technical specifications and prior ownership unverified')])
    c.finish('ecrunch-I610661', 1)
    return 1
