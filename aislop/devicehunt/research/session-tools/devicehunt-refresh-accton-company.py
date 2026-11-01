from pathlib import Path
source=Path('/tmp/devicehunt-refresh-rra-spellings.py').read_text()
source=source.replace("'rra-spellings-readonly-check.json'", "'accton-company-readonly-check.json'")
source=source.replace("[('BARRELEYE','lookup-rra-barreleye'),('SAT1A_C2','lookup-rra-sat1a')]", "[('AC-IG2004W','lookup-rra-accton-singapore')]")
exec(compile(source,'/tmp/devicehunt-refresh-rra-spellings.py','exec'))
