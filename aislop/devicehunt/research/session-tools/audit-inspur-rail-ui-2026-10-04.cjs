const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const {mount} = require('../../tests/report.test.cjs');
const root = path.resolve(__dirname, '../..');
const html = fs.readFileSync(path.join(root, 'data/exports/catalog.html'), 'utf8');
const template = fs.readFileSync(path.join(root, 'devicehunt/report.html'), 'utf8');
const script = value => value.match(/<script>\s*([\s\S]*?)<\/script>/)[1];
assert.equal(script(html), script(template));
const data = JSON.parse(html.match(/<script id="data" type="application\/json">([\s\S]*?)<\/script>/)[1]);
assert.equal(data.length, 3976);
const ui = mount(data);
const count = () => Number(ui.nodes.count.textContent.match(/^\d+/)[0]);
ui.set('vendor', 'Inspur'); assert.equal(count(), 3);
ui.set('scope', 'all'); assert.equal(count(), 47);
ui.set('query', 'NS5162M5'); assert.ok(ui.titles().includes('Inspur NS5162M5'));
ui.set('query', 'S6550V2-48TQ-AC/D'); assert.deepEqual(ui.titles(), ['Inspur S6550V2-48TQ-AC/D']);
ui.set('scope', 'manufacturer'); assert.equal(count(), 0);
ui.set('query', ''); ui.set('vendor', 'Inventec'); assert.equal(count(), 44);
ui.set('scope', 'all'); assert.equal(count(), 79);
ui.set('vendor', 'ByteDance'); assert.equal(count(), 0);
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(path.join(root, file))).digest('hex');
fs.writeFileSync(path.join(root, 'data/exports/inspur-rail-ui-audit-2026-10-04.json'), JSON.stringify({
  database_sha256: hash('data/devices.sqlite'), html_sha256: hash('data/exports/catalog.html'),
  generated_data_identities: data.length,
  verified_counts: {inspur_manufacturer: 3, inspur_all: 47,
    grouped_switch_manufacturer: 0, grouped_switch_all: 1,
    inventec_manufacturer: 44, inventec_all: 79, bytedance_all: 0},
  method: 'Generated HTML payload and matching inline script in DOM harness; no browser screenshot',
  raw_feed_scope: '1064 mixed support entries remain outside the device inventory',
}, null, 2) + '\n');
