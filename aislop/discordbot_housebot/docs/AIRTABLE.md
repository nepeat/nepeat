# Airtable sync (optional, best effort)

housebot can mirror each listing into the Airtable base `appc5LQi7Uo9Y75yN`.
It is **off until configured**, it **assumes nothing about your schema**, and it
**never blocks a Discord command**. When unconfigured, the sync no-ops with a
traceable reason recorded in the `airtable_sync` D1 table.

## Why a field map is required

housebot does not know your column names, and guessing would either write to the
wrong field or fail every request. So it exports *logical* keys and you map the
ones you want onto real Airtable field ids or names.

Logical keys (`src/airtable/fieldmap.ts`):

`key` · `url` · `provider` · `listingId` · `status` · `price` · `address` ·
`city` · `state` · `zip` · `beds` · `baths` · `sqft` · `lotSqft` · `yearBuilt` ·
`hvacUnverified` · `threadUrl` · `lastCheckedIso`

`key` is **required** — it is the canonical `provider:listingId` used as the
upsert merge field. Unmapped keys are simply not sent. Unknown keys in your JSON
are ignored and reported.

## Discovery step (do this once)

1. Create a personal access token at <https://airtable.com/create/tokens> with
   scopes `data.records:read`, `data.records:write`, and `schema.bases:read`,
   granted on base `appc5LQi7Uo9Y75yN`.
2. List the tables and fields:

   ```bash
   curl -sS -H "Authorization: Bearer $AIRTABLE_TOKEN" \
     "https://api.airtable.com/v0/meta/bases/appc5LQi7Uo9Y75yN/tables" \
     | jq '.tables[] | {id, name, fields: [.fields[] | {id, name, type}]}'
   ```

3. Pick the table (`tbl…` id is more stable than the name) and note the field
   ids you want to write.
4. Make sure the `key` column is a **single line text** field with a **unique**
   value per house. Airtable's upsert merges on value equality; a non-unique key
   column will merge unrelated rows.

## Configure

```bash
npx wrangler secret put AIRTABLE_TOKEN
npx wrangler secret put AIRTABLE_TABLE            # e.g. tblAbC123XyZ
npx wrangler secret put AIRTABLE_FIELD_MAP_JSON   # the JSON below, one line
```

```json
{
  "key": "fldKeyXXXXXXXXXX",
  "url": "fldUrlXXXXXXXXXX",
  "status": "fldStatusXXXXXXX",
  "price": "fldPriceXXXXXXXX",
  "address": "fldAddrXXXXXXXXX",
  "beds": "fldBedsXXXXXXXXX",
  "baths": "fldBathsXXXXXXXX",
  "sqft": "fldSqftXXXXXXXXX",
  "threadUrl": "fldThreadXXXXXXX",
  "lastCheckedIso": "fldCheckedXXXXXX"
}
```

`AIRTABLE_BASE_ID` is already set as a var in `wrangler.jsonc`.

## Behavior

- One `PATCH /v0/{base}/{table}` per successful snapshot, with
  `performUpsert.fieldsToMergeOn = [<your key field>]` and `typecast: true`.
  Repeated syncs of an unchanged listing are idempotent.
- 6 second timeout; failures return an `error` result instead of throwing.
- Tokens are redacted from every returned detail string (`pat…` → `pat***`).
- Results land in `airtable_sync (listing_key, record_id, last_status,
  last_detail, last_synced_at)`.

Check what happened:

```bash
npx wrangler d1 execute housebot --remote \
  --command "SELECT listing_key, last_status, last_detail, last_synced_at FROM airtable_sync ORDER BY last_synced_at DESC LIMIT 20"
```

Typical `last_status` values: `ok` (created/updated), `skipped` (not configured
— the detail says which piece is missing), `error` (HTTP or network, detail
redacted).

## Not verified

No live Airtable call has been made from this repo — no token was available. The
client is covered by unit tests against a mocked endpoint (request shape, upsert
merge field, skip reasons, error handling, token redaction), which is *not* the
same as confirming your base accepts these writes. Run one `/house add` after
configuring and check `airtable_sync` before trusting it.
