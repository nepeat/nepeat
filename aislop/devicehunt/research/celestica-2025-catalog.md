# Celestica 2025 platform catalog

The original [eight-page PDF](https://www.celestica.com/uploadedFiles/artifact/OSS_2025_Catalog.pdf)
is cached as `data/raw/celestica-2025-catalog.pdf`, with a hashed embedded-text
capture and import bundle. Product cards on pages 4 and 5 were visually reviewed.
All 17 cards are imported as 19 exact model assertions:

- Page 4: DS4000, DS4001, DS3001, DS3000, DS5000, DS4101, DS4100,
  ES1010, ES1050, EG1050, ES1500, ES1000.
- Page 5: SC6100, SC4200, SD6200, SD4100, DS2000, DS1000, SE4200.

These models already existed in the catalog from other sources. The improvement
is original, locally verifiable brand and product-description evidence, not an
increase in unique device count. The old web-only SC4200 seed is removed from
`data/curated.json`; its source ID is now owned by this PDF importer. Ordinary
curated refreshes no longer overwrite the stronger source.

Product descriptions retain alternatives and grouped headings. DS4000/DS4001
and ES1010/ES1050 remain separate identities with the same scoped card description;
they are not aliases. Port/speed alternatives are not allocated to individual
models or converted into invented ordering codes. Form factor is searchable with
`specs`; full product descriptions and original headings are available in SQL.
The extraction repeats “Controller” in SC4200's caption. The PDF image shows it
once; the original extracted text and an explanation are both retained.

Brand is the evidenced role. The catalog's company-level hyperscaler marketing
is not assigned to any model, and no physical factory, particular operator,
current stock, installed drives or firmware entitlement is inferred. “New” and
“now shipping” graphics are not imported as current availability. Coverage is
this specific 2025 portfolio, not all historical Celestica hardware.

```sh
python3 -m devicehunt import celestica-catalog-pdf data/raw/celestica-2025-catalog-bundle.json
python3 -m devicehunt specs SC4200 --exact-model
python3 -m devicehunt sql "SELECT model,name,value,source_url FROM device_facts WHERE source_id='celestica-2025-catalog' ORDER BY model,name"
```

The importer uses only the standard library on the cached captures. Creating a
fresh capture requires pypdf and visual checking of the product cards. It checks
hash association, eight pages, edition text, card counts, duplicate models,
model/description categories and grouped-model totals before replacing a source.
The original bytes, text capture and bundle are included in the offline rebuild
manifest. The export `data/exports/celestica-2025-catalog.csv` contains all 19
model descriptions and page URLs.

## Remaining baseline search

Exact-label searches for `7760-32X-A-AC-F` still returned seller listings rather
than independent source evidence. AZ4444 results included the already known
suffixed regulatory entries. A UL Brazilian listing (BR4384, Openglobe) also
lists full variants, not sufficient bare-model/Edge-Core evidence. No new alias
or validation pass was created. The previously identified AS7760 family manual
still cannot establish the complete 7760 ordering code by itself.
