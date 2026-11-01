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
assert.equal(data.length, 3942);
const ui = mount(data);
const count = () => Number(ui.nodes.count.textContent.match(/^\d+/)[0]);
ui.set('vendor', 'Inventec'); assert.equal(count(), 44);
ui.set('scope', 'all'); assert.equal(count(), 79);
ui.set('query', 'Metagross'); assert.deepEqual(ui.titles(), ['Inventec Metagross']);
ui.set('scope', 'manufacturer'); assert.equal(count(), 0);
ui.set('query', ''); ui.set('vendor', 'Inspur'); assert.equal(count(), 3);
ui.set('scope', 'all'); assert.equal(count(), 13);
ui.set('query', 'NF5280M5'); assert.deepEqual(ui.titles(), ['INSPUR NF5280M5']);
ui.set('query', ''); ui.set('vendor', 'ByteDance'); assert.equal(count(), 0);
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(path.join(root, file))).digest('hex');
fs.writeFileSync(path.join(root, 'data/exports/storage-inspur-ui-audit-2026-10-04.json'), JSON.stringify({
  database_sha256: hash('data/devices.sqlite'), html_sha256: hash('data/exports/catalog.html'),
  generated_data_identities: data.length,
  verified_counts: {inventec_manufacturer: 44, inventec_all: 79,
    metagross_manufacturer: 0, metagross_all: 1, inspur_manufacturer: 3,
    inspur_all: 13, bytedance_all: 0},
  method: 'Generated HTML payload and matching inline script in DOM harness; no browser screenshot',
  raw_feed_scope: '1064 mixed support entries remain outside the device inventory',
}, null, 2) + '\n');
