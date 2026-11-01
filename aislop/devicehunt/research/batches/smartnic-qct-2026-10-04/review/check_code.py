from pathlib import Path
import json,hashlib
root=Path('.'); eng=Path('research/batches/smartnic-qct-2026-10-04/engineering');sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
hashes=json.loads((eng/'code-hashes.json').read_text())
for path,h in hashes.items():assert sha(Path(path))==h,path
proof=json.loads((eng/'equivalence.json').read_text());reports=proof['reports'];assert len(reports)==98
for name,item in reports.items():
 a=eng/'isolatedreports'/name;b=Path('data/exports')/name;assert a.read_bytes()==b.read_bytes();assert sha(a)==item['sha256']
result={'code_hashes':hashes,'independently_verified_serialized_reports':len(reports),'live_sha256':sha(Path('data/devices.sqlite')),'selective_tests':'tests.test_lookup_batch: 5 passed','author_full_suite':'433 pass, full-tests.log','benchmark':json.loads((eng/'benchmark.json').read_text())}
Path('research/batches/smartnic-qct-2026-10-04/review/code-checks.json').write_text(json.dumps(result,indent=2)+'\n');print('verified frozen code and 98 serialized reports')
