from pathlib import Path
source=Path('/tmp/devicehunt-compare-rra-spellings.py').read_text().replace('data/rebuilt-with-rra-spellings.sqlite','data/rebuilt-with-picos.sqlite')
exec(compile(source,'/tmp/devicehunt-compare-rra-spellings.py','exec'))
