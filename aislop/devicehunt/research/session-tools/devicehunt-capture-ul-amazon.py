import json
from pathlib import Path
from devicehunt import ul_amazon as ul
entries='''TE1AXYZ
Frederic_Mistral-xxx
Frederic_Mistral-xxx
AmbroseXXF*****
Charles_Richet-xxx
Ambrose*****
Emerald
Camillo_Golgi-xxx
Peridot
Artemis
Camillo_Golgi
Chimera
Mustang
Sasquatch
Atlanta
Waynemanor
Centaurus
Tetra
Rainier
Pyro
ArtemisH
ChimeraH
Trek
Rattlesnake
Pulsar
Charles_Richet-xxx
Rudyard_Kipling
Rudyard_Kipling-xxx
CMWA#6400-XYZ
Catalina
Tetra
Blackfoot19
Tiger_Woods-xxx
Tiger_Woods
TETONJ
GrandTetonj
TETONH
GrandTetonh
Boxer
Arnold_Palmer-XXX
S22708FGP
C22201FQU
Timberwolf
Ambrose76
Wukong
Frederic_Mistral-xxx
Charles_Richet-xxx
Ambrose87
Frederic_Mistral-xxx
Charles_Richet-xxx
Boxer2
HopperH
CS42_1RP_G2
CS48_2RP_G2
BARRELEYE MGMT
BARRELEYE SWITCH
BARRELEYE COMPUTE
CS48_4LC_E1
AZ9075-64X-DC-42
Monza
AZ9074-64X-DC-42
CS48_2RP_E1
CS48_2WM_E1
CS42_1RP_E1
CS42_1WM_E1
CS42_2WM_E1
Hotdog
HP87j
HP89j
Hotdog1U5
SCF10_E1
VCF10_E1
CS48_0DC_E1
NAF20_L1'''.splitlines()
models,patterns=ul.extract(entries)
data=dict(schema_version=1,capture_format='web_search_reviewed_model_entries',source_url='https://productiq.ulprospector.com/en/profile/2859718/azot7.e515139?page=75&term=AZOT7',retrieved_at='2026-09-27 05:45:10 UTC',document=ul.DOCUMENT,listing_holder=ul.HOLDER,section='Models',extraction_scope=ul.SCOPE,model_section_entries=entries,models=sorted(models),unexpanded_patterns=sorted(patterns),scope='Factual Models entries from the public web-search result, reviewed into individual names. Duplicates and case preserved. Direct opening returned 403; original HTML and original grouping unavailable. Search freshness and full coverage unverified; UL states models may be products or components and its public listing may omit models. Templates are not expanded, exact names are not cross-brand aliases, and listing holder does not establish physical manufacturer or deployment.')
Path('data/raw/ul-amazon-platforms.json').write_text(json.dumps(data,indent=2)+'\n')
print(len(entries),'entries;',len(models),'exact names;',len(patterns),'unique patterns')
