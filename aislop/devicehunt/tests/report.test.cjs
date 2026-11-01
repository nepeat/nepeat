// Exercise the actual inline report script without a browser or network access.
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const template=fs.readFileSync(path.join(__dirname,'../devicehunt/report.html'),'utf8');
const script=template.match(/<script>\s*([\s\S]*?)<\/script>/)[1];

function mount(devices){
 const nodes={};
 class Element {
  constructor(tag){this.tag=tag;this.children=[];this.value='';this.checked=false;this.events={};}
  set id(id){nodes[id]=this;}
  setAttribute(name,value){this[name]=value;}
  append(...children){this.children.push(...children);}
  after(node){this.next=node;}
  replaceChildren(...children){this.children=children;}
  addEventListener(event,handler){this.events[event]=handler;}
 }
 for(const id of ['data','query','vendor','hyper','results','more','count'])new Element('div').id=id;
 nodes.data.textContent=JSON.stringify(devices);
 const document={getElementById:id=>nodes[id],createElement:tag=>new Element(tag),createTextNode:text=>({textContent:text})};
 vm.runInNewContext(script,{document});
 return {
  nodes,
  set(id,value){nodes[id].value=value;nodes[id].events.input();},
  titles(){return nodes.results.children.map(card=>card.children[0].textContent);}
 };
}
function device(title,affiliations){
 return {id:title,title,model:title,kind:'switch',affiliations,identifiers:[],facts:[],hyperscaler_evidence:[],fcc_exhibits:[],shopping:[]};
}
const affiliation=(company,role,target=company)=>({company,role,target,source_url:'https://example.org/evidence',
 source_tier:'vendor',source_snapshot_eligible:true,
 evidence_class:role==='listed_factory'?'factory_listing':role==='manufacturer'?'manufacturer_claim':'other_association'});

test('company and role must belong to the same affiliation',()=>{
 const ui=mount([
  device('factory record',[affiliation('Celestica','listed_factory'),affiliation('Cisco','manufacturer',null)]),
  device('manufacturer record',[affiliation('Celestica','manufacturer')])
 ]);
 ui.set('scope','all');
 ui.set('vendor','Celestica');
 assert.equal(ui.titles().length,2);
 ui.set('role','manufacturer');
 assert.deepEqual(ui.titles(),['manufacturer record']);
 ui.set('role','listed_factory');
 assert.deepEqual(ui.titles(),['factory record']);
 ui.set('vendor','');
 ui.set('role','manufacturer');
 assert.equal(ui.titles().length,2); // With no company selected, Cisco is eligible too.
});

test('roles come from the snapshot and compose with text and hyperscaler filters',()=>{
 const item=device('AF1',[affiliation('Amazon Data Services','manufacturer')]);
 item.hyperscaler_evidence=[{operator:'AWS',relationship:'product_family',detail:'fixture',source_url:'https://example.org'}];
 const ui=mount([item,device('AF2',[affiliation('Amazon Data Services','manufacturer')])]);
 assert.deepEqual(ui.nodes.role.children.map(option=>option.value),['','manufacturer']);
 ui.set('role','manufacturer');
 ui.nodes.hyper.checked=true;ui.nodes.hyper.events.input();
 assert.deepEqual(ui.titles(),['AF1']);
 ui.set('query','AF2');
 assert.deepEqual(ui.titles(),[]);
 ui.nodes.hyper.checked=false;ui.nodes.hyper.events.input();
 assert.deepEqual(ui.titles(),['AF2']);
});

test('changing role resets pagination and an empty snapshot remains usable',()=>{
 const ui=mount(Array.from({length:60},(_,i)=>device('model '+i,[affiliation('Oracle','brand')])));
 ui.set('scope','all');
 assert.equal(ui.titles().length,50);
 ui.nodes.more.events.click();assert.equal(ui.titles().length,60);
 ui.set('role','brand');assert.equal(ui.titles().length,50);
 assert.equal(ui.nodes.more.hidden,false);
 const empty=mount([]);
 assert.deepEqual(empty.titles(),[]);
 assert.equal(empty.nodes.more.hidden,true);
});

test('manufacturer default excludes seller, unhashed, brand and non-target claims',()=>{
 const ui=mount([
  device('external',[affiliation('Oracle','manufacturer')]),
  device('seller',[{...affiliation('Oracle','manufacturer'),source_tier:'marketplace',source_snapshot_eligible:false}]),
  device('unhashed',[{...affiliation('Oracle','manufacturer'),source_snapshot_eligible:false}]),
  device('brand',[affiliation('Oracle','brand')]),
  device('other',[affiliation('Cisco','manufacturer',null)])
 ]);
 assert.equal(ui.nodes.scope.value,'manufacturer');
 assert.deepEqual(ui.titles(),['external']);
 ui.set('scope','all');assert.equal(ui.titles().length,5);
 ui.set('vendor','Oracle');assert.equal(ui.titles().length,4);
 ui.set('scope','manufacturer');assert.deepEqual(ui.titles(),['external']);
});

test('scope, target, role and source eligibility cannot borrow another affiliation',()=>{
 const ui=mount([
  device('shared',[affiliation('Oracle','brand'),affiliation('Celestica','manufacturer')]),
  device('factory',[affiliation('Celestica','listed_factory'),affiliation('Cisco','manufacturer',null)]),
  device('split',[{...affiliation('Oracle','manufacturer'),source_snapshot_eligible:false},affiliation('Oracle','brand')])
 ]);
 ui.set('vendor','Oracle');assert.deepEqual(ui.titles(),[]);
 ui.set('vendor','Celestica');assert.deepEqual(ui.titles(),['shared']);
 ui.set('scope','factory');assert.deepEqual(ui.titles(),['factory']);
 ui.set('role','manufacturer');assert.deepEqual(ui.titles(),[]);
 ui.set('role','listed_factory');assert.deepEqual(ui.titles(),['factory']);
 ui.set('query','shared');assert.deepEqual(ui.titles(),[]);
});

test('scope switches reset pagination while all associations keeps every identity',()=>{
 const ui=mount(Array.from({length:60},(_,i)=>device('model '+i,[affiliation('Oracle','manufacturer')])));
 ui.nodes.more.events.click();assert.equal(ui.titles().length,60);
 ui.set('scope','all');assert.equal(ui.titles().length,50);
 ui.set('scope','factory');assert.deepEqual(ui.titles(),[]);
 assert.equal(ui.nodes.more.hidden,true);
 ui.set('scope','manufacturer');assert.equal(ui.titles().length,50);
 const empty=mount([device('no affiliation',[])]);
 assert.deepEqual(empty.titles(),[]);
 empty.set('scope','all');assert.deepEqual(empty.titles(),['no affiliation']);
});

// Used by the live snapshot audit to exercise exactly the same inline script.
module.exports={mount};

test('shopping links render literal rows and empty arrays have no shopping group',()=>{
 const hardware=device('hardware',[]);
 hardware.shopping=[{query:'NF5280M6',ebay:'https://www.ebay.com/sch/i.html?_nkw=NF5280M6',
  web:'https://www.google.com/search?q=%22NF5280M6%22+used+surplus'}];
 const software=device('software reference',[]);
 const blank=device('no usable query',[]);
 const ui=mount([hardware,software,blank]);ui.set('scope','all');
 const groups=ui.nodes.results.children.map(card=>card.children.filter(child=>
  child.tag==='details'&&child.children[0]?.textContent==='marketplace search terms'));
 assert.deepEqual(groups.map(g=>g.length),[1,0,0]);
 const row=groups[0][0].children[1];
 assert.equal(row.textContent,'NF5280M6 · ');
 assert.deepEqual(row.children.filter(child=>child.tag==='a').map(a=>[a.textContent,a.href]),
  [['eBay',hardware.shopping[0].ebay],['web / surplus',hardware.shopping[0].web]]);
});
