# installed local PDF runtime — 2026-10-12

outcome: complete, usable **Poppler 26.06.0** found in a realized Nix store
closure outside PATH. no installs, downloads, builds, network or system changes.
four bounded inspection/demo commands found and demonstrated the runtime;
one subsequent output check confirmed seven nonempty page-text segments.

executables:

```
/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin/pdfinfo
/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin/pdftotext
/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin/pdftoppm
```

each `-v` invocation returned version 26.06.0 and exit status zero.
`runtime-report.json` records exact argv, return codes, stdout/stderr,
timestamps, source hashes and output sizes/hashes.

inspection scope: known profile/virtualenv paths and PATH names, then only
shallow /nix/store entry names and fixed bin/site-packages children.
no recursive filesystem crawl. direct store executable invocation does not
run nix shell/build/eval or fetch a missing closure.

demonstration source: a byte-identical copy of the saved seven-page CSA PDF.
original remained SHA-256
`c7d1e731170257bfbddb3989b43beb49e04dbc95bbddf76cfa9b2acb333fdc60`
before and after. `pdfinfo` returned seven pages. `pdftotext -layout`
extracted seven nonempty form-feed-delimited segments. one render used
`pdftoppm -f 4 -l 4 -singlefile -scale-to 1800 -png`.

| artifact | SHA-256 |
| --- | --- |
| `bel-pfe1500-csa-demo.pdf` | `c7d1e731170257bfbddb3989b43beb49e04dbc95bbddf76cfa9b2acb333fdc60` |
| `bel-pfe1500-csa-poppler-layout.txt` | `fa1d1ec95b37a620e91ea28fe6e73d8d56773919c34f0ce0dcfdd06a860c5c23` |
| `bel-pfe1500-csa-page4.png` | `7eef9de3435ced3594258a22f616e0bee891f103c16d3ba0b05c2da409eee197` |

rendered page path for coordinator visual review:
`/home/erin/nepeat/aislop/devicehunt/research/batches/discovery-2026-10-12/pdf-runtime/bel-pfe1500-csa-page4.png`.
the PNG is readable and displays the printed page 4 CSA table. the runtime
worker inspected it to confirm successful rendering; independent claim review
belongs to the coordinator.

import boundary: this is **Poppler text, no OCR**, not pypdf. the current
`devicehunt.documents.read_pages` requires literal extraction method
`pypdf embedded text; no OCR`, matching PDF hash and a complete consecutive
page list; `reference_documents.prepare` additionally validates sibling
dependencies and declared page count. don't relabel this demo to satisfy that
contract.

no realized pypdf/PyPDF2/pymupdf candidate was found in the specific shallow
package-name scan. two realized pdfminer-six candidates surfaced but were
not loaded or demonstrated; runtime finding ended after decisive Poppler
availability. no global absence is asserted.

next bounded step: use this installed renderer for the two remaining PSU
visual checks. broader extraction/import compatibility remains a separate
assigned stage; no importer/schema changes here.
