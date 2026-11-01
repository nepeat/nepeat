# full-access worker replacement checkpoint

owner `/root/practical_sources`; checkpoint requested by root. **all network
collection is complete and closed; offline extraction/novelty deliverables are
unfinished.** this worker stops writing after this checkpoint. transfer exclusive
ownership of this directory to the replacement worker before continuing.

## scope and permissions

owned path only:
`research/batches/discovery-2026-10-14/odm-source-expansion/`.
shared code/catalog/manifests/exports read-only. no installs, imports, delegation,
contact or publication. raw names/links are discovery, not identity/manufacturer/
deployment/fitment claims. root requested replacement because this worker still
sees old workspace-write/network-restricted developer instructions while parent
now reports danger-full-access/network-enabled/approval-never. prior requests
used approved escalation after this environment's known sandbox DNS restriction.
the replacement must follow its own current developer permission policy.

coordinate source selection with `/root/wave5_review`: that worker owns
Celestica/Edge-Core/Oracle/Accton bulk primary catalogs; these new NVIDIA/AMD/MiTAC
datasets do not overlap. parent has already been told this selection.

## finite budget and access outcomes

**six web search queries used, cap eight. ten actual HTTP exchanges used, cap
twelve.** searches selected the exact planned routes; no searches are needed to
finish. three fresh host robots policies returned200 and allowed all exact paths,
with no crawl delay. six planned resources succeeded after one literal same-origin
NVIDIA301 redirect. no denial, timeout, login, throttle, retry, bypass or alternate
route occurred. no further remote calls are needed or authorized merely because
two exchanges/searches remain unused.

`capture-complete.json` closes the source set and makes `fetch.py` refuse more
requests. `requests.jsonl` has full URL/argv/time/status/body/header/hash/policy
receipts for all ten exchanges. source bytes and response headers are immutable.

1. `https://docs.nvidia.com/robots.txt`,200.
2. `https://docs.nvidia.com/certification-programs/latest/nvidia-certified-systems.html`,200;
   `nvidia-certified.html`,241183bytes,
   SHA`7704086f4984d21634814f6b75146ee278bc0863d3848d476d501270d4000f25`.
3. `https://docs.nvidia.com/certification-programs/pdf/ngc-ready-systems.pdf`,301;
   `nvidia-ngc-ready.pdf` is the **empty redirect response**, not a PDF document.
4. literal same-origin Location
   `https://docs.nvidia.com/certification-programs/latest/ngc-ready-systems.html`,200;
   **`redirect-nvidia-ngc-ready.pdf` actually contains HTML**,72825bytes,
   SHA`ad29b7e9933d8bb45d971ae0dcdf8582b784afc464a16d1df9959a903b7c93fd`.
   keep its immutable filename; route extraction by content/headers, not extension.
5. `https://www.amd.com/robots.txt`,200.
6. `https://www.amd.com/content/dam/amd/en/documents/products/accelerators/instinct/amd-instinct-solution-catalog.pdf`,200;
   `amd-instinct-catalog.pdf`,365195bytes,
   SHA`8436689a7a45614e4b4078e2bd13ad6aba0403d0407d5d4f79f4b557203c2d5c`.
7. `https://www.mitaccomputing.com/robots.txt`,200.
8. `https://www.mitaccomputing.com/products/general-purpose-servers`,200;
   `mitac-general.html`,277058bytes,
   SHA`9e2c8e54bfa1450c6e0df37ecd210702048bdf4e105a96293ebcaadabc6f7a8f`.
9. `https://www.mitaccomputing.com/products/ai-servers`,200;
   `mitac-ai.html`,289277bytes,
   SHA`60de50260bcb2cb78ad085e15d78b2df54826244ad88613d5e0e005abf5eed3d`.
10. `https://www.mitaccomputing.com/products/multi-node-servers`,200;
    `mitac-multi.html`,318733bytes,
    SHA`a2ed71449f6bf9c7aef8a78004c2e3ec737e11b6564b6da19c048785060f4222`.

four selected bulk datasets: current NVIDIA certified systems; historical
NGC-Ready systems; AMD Instinct solution catalog; MiTAC product-index dataset
(three categories). `plan.json` preserves exact routes, search provenance and
exclusions. `prior-coverage-pins.json` pins read notes and manifest at selection.

known prior access failure: `research/ocp-product-evidence.md` explicitly records
full OCP product index HTTP403 and legacy networking-wiki403. neither was
requested; do not use an alternate route. existing OCP selected12 product records
are already imported. ONIE/ONL/SONiC/PDDF/Stratum/DENT/OpenBMC/entity-manager/
MetaOpenBMC/coreboot/LinuxDMI/NetBox/SPEC/Ubuntu/RedHat/EnergyStar were checked
against current manifest/2026-10-11 notes and not reacquired. initial read-only
live source query found no AMD/NVIDIA source URLs; only OCP reviewed source and
one minipack2 PDF among those queried domains. no claim of global raw-file absence.

## code/results state and next offline step

only acquisition prototype `fetch.py` exists so far; **the reusable bulk extractor,
raw discovery rows, novelty report, final handoff and artifact manifest have not
been written**. full-catalog capture is real, not a plan-only result.

unchanged reviewed `scripts/extract_pdf_pages.py` completed the AMD PDF into
`amd-instinct-pages.json`: **all six pages**, source hash bound, method Poppler
pdftotext-layout embedded text/noOCR; installed tool pair26.06.0 at
`/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin`.
no local renders yet. six-page raw text includes many partners beyond the original
scope, including MiTAC, Wistron, Ingrasys, Supermicro, GIGABYTE, ASRockRack,
ASUS, AMAX, Compal and others. parent/group cells are vertically centered in
PDF tables: **do not blindly forward-fill partner names between product rows**.
preserve exact raw line, page/line coordinates, column boundaries and blank cells;
label partner mapping as unreviewed unless visually established. retain printed
oddities rather than correcting them (e.g. `AS -...`, `EXAACT`, `MI2104`).

next command should be **offline inspection/parser authoring**, e.g. read saved
HTML with stdlib `HTMLParser`; no package installation. use `python3 -B` or
`PYTHONDONTWRITEBYTECODE=1` to avoid bytecode writes outside this owned directory.

- NVIDIA HTML has ordinary server-rendered tables. preserve each table ID,
  nearby heading/caption, original `<th>` labels, complete literal row cell
  text/attributes/links, row source lines, duplicates and blank cells. current
  table IDs include `table-data-center-servers`, reference-configuration tables,
  edge/IGX/VDI/workstation/Arm tables. first column usually `Partner`; second
  column varies(`System`, `Data Center Server`, etc.). retain complete table rows;
  mark relevant-partner selections without discarding full raw output. Inventec,
  QCT, PEGATRON, Wistron and Supermicro appear in current list; historical NGC
  includes Inventec/Wiwynn/QCT/Inspur/MiTAC and more. don't imply the historical
  list is a current qualification guarantee.
- MiTAC HTML is minified Nuxt SSR, mostly one giant line; don't print raw lines
  with rg because output is huge. visible product cards use `h3` exact code under
  enclosing `.swiper-slide`. for `B8056G68AE12HR`, ancestor chain ends
  `.swiper-slide` > `.group...` > `.relative...` > `.flex...` > `.swiper-no-swiping...`
  > `h3`. category/family `h2` and card label/value text plus exact `Learn more`
  hrefs are present in the captured HTML. preserve family/category context,
  source byte/column offsets or node hashes, and explicit `/ per node` qualifiers.
  three pages overlap by family/configuration; retain row-source multiplicity.
- AMD: start from the complete sidecar; first three pages have product columns
  beginning around27characters, p6 around30; derive actual header positions per
  page/table rather than assuming one fixed layout. raw discovery can safely
  retain literal product-line records without asserting merged partner mappings.
- read-only live novelty: compare exact printed partner/name pairs and model
  strings versus devices/identifiers/sourceURLs, keep exact namespace hits versus
  same-model/different-brand leads distinct. no alias normalization/import. record
  live DB SHA/time and stable read-only transaction so parent integration changes
  do not masquerade as source novelty. save all results in this owned directory.

finish with reusable offline extractor code, raw discovery rows, schema/scope
notes, SHA receipts, live novelty report and compact handoff. production/shared
state remains read-only. no curator/candidate/import is assigned in this breadth
task. earlier practical-source `../practical-sources/` is complete and outside
this replacement's write ownership unless root explicitly transfers it.

## live handles

**none.** final network PTY session98480 completed exit0 and was polled; extraction
command completed exit0. no tools/processes are running or awaiting resume.
