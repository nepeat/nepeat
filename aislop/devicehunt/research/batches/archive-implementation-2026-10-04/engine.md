# archive engine handoff

outcome: milestones 1–2 engine complete pending independent acceptance. only the
assigned modules/tests and this note were written; no existing captures, catalog,
manifest, exports or shared archive were mutated. all tests use temporary dirs.

`devicehunt.artifacts.Archive(root, workspace=None, create=False, readonly=False)`
uses immutable `objects/<first-two-hash-characters>/<sha256>` and JSON receipts;
`index.sqlite` is disposable. `.db` is available for operational tables, which
`reindex()` leaves intact. explicit `create=True` is required to reconstruct a
missing index, and missing roots/indexes otherwise fail. default workspace is
archive root's parent; integration should always supply its actual workspace.

- `register_file(path, expected_sha256=None, copy=False)` returns a hash. ordinary
  registration records a workspace-relative legacy location without copying;
  `copy=True` publishes independently copied bytes, never a legacy hardlink.
- `store_bytes(bytes)` returns a hash, publishing one immutable content object.
- `resolve(hash)` returns a verified existing location; `object_path(hash)` gives
  the canonical object path without promising existence.
- `receipt(kind,payload,receipt_id=None)` returns ID; `get_receipt(id)` and
  `receipts(kind=None)` return wrappers containing `id/kind/created_at/payload`.
  default IDs represent distinct actual events; deterministic registration IDs
  reuse original frozen wrappers. operational SQLite timestamps are not evidence.
- `audit()` / `verify()` return `ok/errors/orphan_objects/staging/receipts`;
  `reindex()` reconstructs all indexed durable metadata, transactionally, from
  receipts. missing/corrupt evidence is reported, never refetched or discarded.

publication writes private staging files, fsyncs complete content, publishes using
an exclusive link, then fsyncs the directory. these are links of a private staging
inode to its final name, never links of legacy files. a crash can leave a pending
file or an object with no receipt; audit reports both and deletes neither.
registration rejects symlinks, traversal, outside-workspace paths, wrong hashes,
nonregular files and observable changes during hashing. objects are chmod 0444;
verification remains mandatory because owner permissions cannot prevent all edits.
receipt payloads remain unchanged; semantic/byte validation of a future accepted
release must additionally pin their hashes. JSON syntax/shape alone does not
establish evidence authenticity.

`Derivations(archive)` provides `recipe(name,'1',configuration=None)`,
`run(recipe_id,inputs,consumer=None,force=False)` and
`register_saved(inputs,output_sha256,method,locator=None)`. inputs are ordered
`{role,sha256}` dictionaries; outputs are role/hash dictionaries. supported local
recipes are `page-text-flatten` (optional string `separator`) and `xml-locations`.
recipes pin actual code hash, Python version, config and schema version. unknown
saved extraction metadata remains null and never matches a new recipe. successful
verified runs are reused by two consumers; consumer receipts preserve distinct
uses without redundant runs. forced runs and failures receive distinct IDs.
missing prior outputs trigger a fresh run; corrupt existing immutable objects
must be recovered explicitly and are never overwritten silently.

checks: `python3 -B -m unittest discover -s tests -p 'test_artifacts.py'` passes
9 tests; corresponding `test_derivations.py` passes 4. coverage includes duplicate
registration/bytes, distinct attempt IDs, changed/corrupt/missing files, unknown
metadata, traversal/symlinks, interrupted publication, orphan reporting,
reconstruction/restoration in a new location, frozen concurrent receipts,
transactional invalid-receipt rejection, concurrent snapshot/write serialization, read-only absence behavior, two consumers,
changed recipe config, reruns, saved-output separation, failed runs and XML
order/duplicate preservation. no network or package installation.

next bounded step: independent engine review and coordinator-owned complete pilot
registration/release integration. saved PDF envelope and reconstruction-chain
semantic validation belongs to the complete pilot/release adapter; this engine
records byte identities and relationships, never infers PDF validity or claims.
