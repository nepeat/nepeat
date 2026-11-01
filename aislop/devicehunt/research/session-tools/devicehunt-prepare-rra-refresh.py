from pathlib import Path
p = Path('/private/tmp/devicehunt-refresh-ghana.py').read_text()
p = p.replace("[('5N4-3H-7DF-22F','lookup-ghana-oracle'),('QFX5100-96S','lookup-ghana-qfx5100')]",
              "[('CP3060-RTM','lookup-rra-main-celestica'),('BayStack 325-24G','lookup-rra-main-accton'),('SFE-CAB','lookup-rra-main-oracle')]")
p = p.replace('ghana-readonly-check.json', 'rra-main-models-readonly-check.json')
Path('/private/tmp/devicehunt-refresh-rra-main.py').write_text(p)
p = Path('/private/tmp/devicehunt-compare-ghana.py').read_text()
p = p.replace('rebuilt-with-ghana.sqlite', 'rebuilt-with-rra-main-models.sqlite')
Path('/private/tmp/devicehunt-compare-rra-main.py').write_text(p)
