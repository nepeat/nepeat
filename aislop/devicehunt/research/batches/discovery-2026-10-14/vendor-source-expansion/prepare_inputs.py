"""Construct a finite hash-pinned config from immutable existing capture manifests."""
from pathlib import Path
import json
from offline_parser import sha, ROOT, HERE


def main():
    edge_manifest = ROOT / 'data/raw/edgecore/products.json'
    edge = json.loads(edge_manifest.read_text())
    dependencies = [dict(path=str(edge_manifest.relative_to(ROOT)), sha256=sha(edge_manifest))]
    inputs = []
    for page in edge['pages']:
        path = ROOT / 'data/raw/edgecore' / page['path']
        if sha(path) != page['sha256']:
            raise ValueError('Edge-Core manifest mismatch: ' + str(path))
        inputs.append(dict(path=str(path.relative_to(ROOT)), sha256=page['sha256'], url=page['url'],
                           status=200, status_basis='successful page listed in immutable existing products.json', format='html'))
    sources = [dict(id='edgecore_product_downloads', source_role='product_page_link_publisher',
                    same_host=True, path_pattern=r'^/download/[^/]+/?$', inputs=inputs)]
    for name, sid, role, fmt, pattern in [
        ('celestica-sitemap.xml', 'celestica_sitemap', 'site_sitemap_publisher', 'xml', None),
        ('oracle-legacy-x86.html', 'oracle_legacy_x86', 'documentation_index_publisher', 'html', r'^/cd/E19121-01/sf\.[^/]+/index\.html$')]:
        receipt_path = HERE / (name + '.receipt.json')
        r = json.loads(receipt_path.read_text())
        if r['status'] != 200 or sha(HERE / name) != r['sha256']:
            raise ValueError('Successful response required')
        dependencies.append(dict(path=str(receipt_path.relative_to(ROOT)), sha256=sha(receipt_path)))
        sources.append(dict(id=sid, source_role=role, same_host=True, path_pattern=pattern,
                            inputs=[dict(path=str((HERE / name).relative_to(ROOT)), sha256=r['sha256'],
                                         url=r['url'], format=fmt, status=r['status'], status_basis='immutable capture receipt')]))
    sparc = []
    for name in ['oracle-older-m-series-full-access.html', 'oracle-older-t-series-full-access.html']:
        rp = HERE / (name + '.receipt.json')
        receipt = json.loads(rp.read_text())
        if receipt['status'] != 200 or sha(HERE / name) != receipt['sha256']:
            raise ValueError('Successful SPARC response required')
        dependencies.append(dict(path=str(rp.relative_to(ROOT)), sha256=sha(rp)))
        sparc.append(dict(path=str((HERE / name).relative_to(ROOT)), sha256=receipt['sha256'], url=receipt['url'], format='html', status=200, status_basis='immutable full-access response receipt'))
    sources.append(dict(id='oracle_older_sparc_indexes', source_role='documentation_index_publisher', same_host=True, required_ancestor_class='linktitlecell', required_anchor_class='tooltip', path_pattern=r'^/cd/[^/]+/index(?:-es)?\.html$', inputs=sparc))
    config = dict(schema_version=2, catalog_path='data/devices.sqlite', dependencies=dependencies, sources=sources,
                  restrictions=[
                      dict(pattern=r'(?:AS7710|AS7760|AS7761)', reason='Conservative model-family quarantine: prior quick-start/certificate retrieval denied; no retry or alternate copy.', evidence='research/owned-hardware-gaps.md; research/batches/networking-primary-2026-10-04/owned/handoff.md; research/as7760-certificate-lead.md'),
                      dict(pattern=r'(?:dcs203.*r0?9|(?:dcs510|as9716).*r0?9|as7312.*r0?2|as4630-54npe.*r0?4)', reason='Conservative revision quarantine: known legacy _upload PDF returned 404; no guessed version/alternate route.', evidence='research/batches/discovery-2026-10-12/next-leads/handoff.md')])
    with (HERE / 'inputs-ready.json').open('x') as f:
        json.dump(config, f, indent=2)
        f.write('\n')
    print('verified', len(inputs), 'Edge-Core page hashes; config created')


if __name__ == '__main__':
    main()
