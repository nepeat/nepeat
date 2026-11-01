from pathlib import Path
source=Path('/tmp/devicehunt-refresh-rra-spellings.py').read_text()
source=source.replace("'rra-spellings-readonly-check.json'", "'ul-amazon-readonly-check.json'")
source=source.replace("[('BARRELEYE','lookup-rra-barreleye'),('SAT1A_C2','lookup-rra-sat1a')]", "[('CS42_1RP_G2','lookup-ul-amazon-cs42'),('AZ9075-64X-DC-42','lookup-ul-amazon-az9075')]")
exec(compile(source, str(Path('/tmp/devicehunt-refresh-rra-spellings.py')), 'exec'))
