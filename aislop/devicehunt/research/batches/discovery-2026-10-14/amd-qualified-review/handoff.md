# AMD qualified-server review — interrupted checkpoint

status: **stopped at the user’s explicit graceful spin-down request**. owner
`/root/vendor_bulk_full`. only this directory was written; no network, original
capture edits, shared code/catalog/manifest/export writes or imports occurred.
actual permissions remained full filesystem access, network enabled, approval
never. **no active processes, exec sessions or pending tool handles remain.**

completed partial work:

- read the original ODM extraction/handoff and its142-fragment limitation.
- rendered **all six PDF pages** at140DPI with existing Poppler; **all six images
  were actually viewed** in the conversation. `render-provenance.json` binds
  source/tool/argv and image hashes. `page-1.png` through `page-6.png` are complete
  page renders. `render.py` reproduces this bounded preparation to new outputs.
- `pages-bbox.xhtml` contains complete Poppler word bounding boxes for six pages;
  it has not been converted into reviewed rows.
- `page-4.svg` is a single exploratory Poppler vector export, invoked as
  `pdftocairo -f 4 -l 4 -svg <original PDF> page-4.svg` with the same installed
  Poppler tool directory as render-provenance. it is an exploratory artifact,
  not a reviewed table extraction; its exact hash is in checkpoint.json.

source PDF, still read-only:
`../odm-source-expansion/amd-instinct-catalog.pdf`, SHA256
`8436689a7a45614e4b4078e2bd13ad6aba0403d0407d5d4f79f4b557203c2d5c`.
original complete sidecar is `../odm-source-expansion/amd-instinct-pages.json`.
original142 product fragments remain unmodified, ungrouped discovery rows.

visual observations requiring a resumed grouping/review step:

- company labels span multiple product rows and are vertically centered;
  ordinary text forward-fill is unsafe. page2/page4 have two tables, while
  pages3,5,6 continue prior sections with fresh explicit company labels.
- multiline product cells exist: page4 HPE CRAY’s EX255a cell includes tray/node
  qualifier lines; page5 HPE’s EX235a cell includes `(water cooled)`. those
  fragments must be retained as complete cells, never separate model identities.
- preserve whole company spellings, including `BULL (FORMERLY EVIDEN)`, `QUANTA
  COMPUTER`, `DELL TECHNOLOGIES`, `HPE CRAY`, and printed `EXAACT CORPORATION`
  versus `EXXACT CORPORATION`; no alias, typo repair or company mapping follows.
- page5 Dell GPU-model entries visibly carry a superscript4; the captured final
  page explicitly explains footnote1 but does not resolve4. do not silently turn
  flattened `MI2104` into an unqualified MI210 claim. qualification uncertainty
  should be an explicit held-row decision.
- page6 `GPU SuperServer SYS-740GP-TNRT` visibly has `3rd Gen Xeon` under the
  AMD Instinct Model column. preserve the source anomaly; do not move it into
  another column or invent a GPU. consider holding that row pending review.
- source footer prints `LE-92707-00 2/26` and copyright2026. treat them as literal
  source date/footer text, distinct from acquisition date/current qualification.
  footnote1 and third-party/no-endorsement disclaimers need row/table scope.

**not completed:** reusable geometric grouping, explicit partner/product span
mapping, complete accepted/excluded row lists, candidate, tests, scratch prepare,
novelty comparison or independent review. no final row/device count is claimed.

safe next bounded step, only after explicit resumption: preserve these artifacts;
use the six already viewed images plus bbox positions to define reviewed table,
column and partner-span boundaries. the exploratory page4SVG exposes explicit
horizontal cell/group borders; test whether those can anchor a conservative
reusable parser before attempting any company association. retain every raw cell
and unresolved qualifier, exclude uncertain rows, and stop after these six pages.
no more collection is needed. root owns current documentation and integration.

`checkpoint.json` contains the exact partial artifact names, sizes, hashes and
terminal handle state. `SHA256SUMS` additionally freezes this checkpoint/handoff.
