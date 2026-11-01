from pathlib import Path
source=Path('/tmp/devicehunt-refresh-rra-spellings.py').read_text().replace("'rra-spellings-readonly-check.json'", "'picos-readonly-check.json'")
source=source.replace("[('BARRELEYE','lookup-rra-barreleye'),('SAT1A_C2','lookup-rra-sat1a')]", "[('AS9737-32DB','lookup-picos-as9737'),('AIS800-32O','lookup-picos-ais800')]")
exec(compile(source,'/tmp/devicehunt-refresh-rra-spellings.py','exec'))
