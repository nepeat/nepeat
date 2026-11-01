from pathlib import Path
source=Path('/tmp/devicehunt-refresh-rra-spellings.py').read_text().replace("'rra-spellings-readonly-check.json'", "'sun-readonly-check.json'")
source=source.replace("[('BARRELEYE','lookup-rra-barreleye'),('SAT1A_C2','lookup-rra-sat1a')]", "[('Netra ct 1600','lookup-sun-netra'),('SUN Blade 2500','lookup-sun-blade')]")
exec(compile(source,'/tmp/devicehunt-refresh-rra-spellings.py','exec'))
