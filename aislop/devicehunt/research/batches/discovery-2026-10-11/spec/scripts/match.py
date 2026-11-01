import re
V=[('Quanta',r'quanta|\bqct\b'),('Inspur',r'inspur'),('IEIT',r'ieit'),('Inventec',r'inventec'),('Wiwynn',r'wiwynn'),
('Foxconn',r'foxconn|hon hai|\bfii\b|ingrasys'),('Hyve',r'\bhyve\b'),('Celestica',r'celestica'),('Oracle/Sun',r'oracle|\bsun\b|sun microsystems'),
('Amazon',r'amazon|\baws\b'),('Accton/Edgecore',r'accton|edge-?core'),('SYNNEX',r'synnex')]
def vend(s):
    return [n for n,p in V if re.search(p,s,re.I)]
