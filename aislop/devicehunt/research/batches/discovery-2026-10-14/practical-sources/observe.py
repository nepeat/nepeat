"""Offline article text and literal table rows only; no requests/imports."""
from pathlib import Path
from html.parser import HTMLParser
import hashlib
import json

HERE = Path(__file__).resolve().parent
def normalized(s): return ' '.join(s.split())

class Article(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.active = False
        self.parts = []
        self.blocks = []
        self.open_blocks = []
        self.tables = []
        self.table = None
        self.row = None
        self.cell = None
    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == 'article': self.active = True
        if not self.active: return
        if tag in ('p', 'h2', 'h3') or tag == 'div' and 'infoboxnote' in attrs.get('class', ''):
            self.open_blocks.append(dict(tag=tag, attrs=attrs, start_line=self.getpos()[0], parts=[]))
        if tag == 'table':
            self.table = dict(start_line=self.getpos()[0], attrs=attrs, rows=[])
        if tag == 'tr': self.row = dict(start_line=self.getpos()[0], cells=[])
        if tag in ('td', 'th'): self.cell = dict(tag=tag, attrs=attrs, start_line=self.getpos()[0], parts=[])
    def handle_data(self, data):
        if not self.active: return
        self.parts.append(data)
        for b in self.open_blocks: b['parts'].append(data)
        if self.cell is not None: self.cell['parts'].append(data)
    def handle_endtag(self, tag):
        if not self.active: return
        if self.open_blocks and self.open_blocks[-1]['tag'] == tag:
            b = self.open_blocks.pop()
            b.update(end_line=self.getpos()[0], text=normalized(''.join(b.pop('parts'))))
            if b['text']: self.blocks.append(b)
        if tag in ('td', 'th') and self.cell is not None:
            c = self.cell
            c.update(end_line=self.getpos()[0], text=normalized(''.join(c.pop('parts'))))
            self.row['cells'].append(c)
            self.cell = None
        if tag == 'tr' and self.row is not None:
            self.row['end_line'] = self.getpos()[0]
            self.table['rows'].append(self.row)
            self.row = None
        if tag == 'table' and self.table is not None:
            self.table['end_line'] = self.getpos()[0]
            self.tables.append(self.table)
            self.table = None
        if tag == 'article': self.active = False

def main():
    for name in ('zs3-2', 'zs3-4'):
        path = HERE/(name+'-internal-components.html')
        body = path.read_bytes()
        parser = Article()
        parser.feed(body.decode('utf-8'))
        result = dict(source_path=path.name, source_sha256=hashlib.sha256(body).hexdigest(), method='stdlib HTMLParser saved article text and literal table-cell rows; no remote images/assets', normalized_article_text=normalized(' '.join(parser.parts)), blocks=parser.blocks, tables=parser.tables)
        (HERE/(name+'-article.json')).write_text(json.dumps(result, indent=2)+'\n')
        (HERE/(name+'-article.txt')).write_text('\n'.join(f"html lines {b['start_line']}-{b['end_line']}: {b['text']}" for b in parser.blocks)+'\n')
        print(f'{name}: {len(parser.blocks)} blocks, {len(parser.tables)} tables')

if __name__ == '__main__': main()
