# additional PSU fact-page visual review

Reviewer `/root` independently viewed six Poppler-rendered PNGs with view_image.
Raw PDF hashes, page selections, image hashes and commands are recorded in
`../psu-facts-review-renders/render-manifest.json`. These are image observations,
not just searches of extracted text. Original captures were unchanged.

- **Nemko P22225905, page 1:** NAC is explicitly included in Model / Type Ref.
  Applicant and Manufacturer are Bel Fuse Inc.; Factory is separately BPS Asia
  Pacific Electronics (Shenzhen) Co., Ltd. The printed mark is bel. AC input
  100–240 V, 12–7 A, 50/60 Hz; main 12 VDC / 125 A; standby alternatives 5 VDC /
  3.3 A or 3.3 VDC / 5 A. The certificate explicitly requires derating at differing
  voltage/ambient; retain issue date as printed 07-01-2022. It also requires
  system-level compliance checks when built in; no current validity/fitment claim.
- **FSP550-29FM, page 1:** heading and product text both give exact -29FM model,
  FSP logo. CRPS 2.0 and PMBus 1.2 are printed features. Input range 90–264 VAC,
  47–63 Hz; the separate output table is scoped to 100–240 V input. Main +12 V
  max 45.8 A, standby +12 Vsb max 2.1 A. Do not add their maxima to infer total
  simultaneous rating. Dimensions explicitly L × W × H = 185 × 73.5 × 39 mm;
  operating temperature 0–55°C. MTBF statement is 500,000 hours at 55°C, 100%
  load. The page's limits/conditions remain distinct from the conflicting -20FM
  heading/drawing on page 2, reviewed separately.
- **PFE1500 series, page 2:** upper ordering group explicitly says programmable
  3.3/5 V standby; exact NAC row states C16 AC connector and ranges 180–350 VDC,
  90–264 VAC. Its N direction is output connector → input AC socket. Lower S412
  group has 12 V standby and must not be transferred to NAC. Preserve datasheet
  operating ranges separately from the certificate's rated input ranges.
- **PFE1100 series, page 1:** 1100 W series class, 12 VDC main, 90–264 VAC,
  programmable 3.3/5 V 16.5 W standby; printed dimensions 321.5 × 54.5 × 40 mm.
  This page does not label those dimension axes. No invented L/W/H annotation.
- **PFE1100 series, page 2:** ordering table says x = N normal / R reverse;
  page 9 explicitly names NA, linking the exact suffix to this series. No
  connector-to-connector airflow direction is given here; do not borrow the
  direction definition from PFE1500. Overview states fan cooling and I2C control.
- **PFE1100 series, page 5:** main-output table gives nominal 12.0 VDC, 1080 W,
  90.0 ADC. General condition is Ta = 0…45°C unless otherwise noted; retain
  voltage/ambient derating caveat from page 9. Normal-airflow standby is 16.5 W
  for both 3.3 V and 5 V settings, unlike the lower reverse-airflow 3.3 V rating.
  Current-limitation thresholds in a separate row are not continuous ratings.
  Footer visibly names Bel Fuse Inc., copyright 2023, revision BCD.00012_AP.

Together with `psu-visual-review.md`, this checks all specifically assigned
fact-bearing pages. It is not all-63-pages visual review or import acceptance.
Full hash-bound extraction and an independent check of the actual candidate
facts are still required. No inference about an auction unit's configuration,
system compatibility, current certification, deployment or physical provenance.
