// Check the actual generated HTML payload and inline script without navigation.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const {mount} = require('../../tests/report.test.cjs');
const root = path.resolve(__dirname, '../..');
const html = fs.readFileSync(path.join(root, 'data/exports/catalog.html'), 'utf8');
const template = fs.readFileSync(path.join(root, 'devicehunt/report.html'), 'utf8');
const inlineScript = value => value.match(/<script>\s*([\s\S]*?)<\/script>/)[1];
assert.equal(inlineScript(html), inlineScript(template));
const data = JSON.parse(html.match(/<script id="data" type="application\/json">([\s\S]*?)<\/script>/)[1]);
assert.equal(data.length, 3941);
assert(!html.includes('__TARGET_NAMES__'));
const ui = mount(data);
const count = () => Number(ui.nodes.count.textContent.match(/^\d+/)[0]);
ui.set('vendor', 'Inventec');
assert.equal(count(), 44);
ui.set('scope', 'all'); assert.equal(count(), 78);
ui.set('query', 'Horsea'); assert.deepEqual(ui.titles(), ['Inventec Horsea']);
ui.set('scope', 'manufacturer'); assert.equal(count(), 1);
ui.set('query', 'NX-H1331'); assert.equal(count(), 0);
ui.set('scope', 'all'); assert.equal(count(), 1);
ui.set('query', 'K905G6_G6P'); assert.deepEqual(ui.titles(), ['Inventec K905G6_G6P']);
ui.set('query', ''); ui.set('vendor', 'Quanta'); assert.equal(count(), 73);
ui.set('scope', 'manufacturer'); assert.equal(count(), 41);
ui.set('scope', 'all'); ui.set('role', 'historical_subsidiary_portfolio');
assert.equal(count(), 9);
ui.set('query', 'QuantaPlex T41S-2U');
assert.deepEqual(ui.titles(), ['QCT QuantaPlex T41S-2U']);
ui.set('role', ''); ui.set('query', '');
ui.set('vendor', 'ByteDance'); assert.equal(count(), 0);
ui.set('vendor', 'Inspur'); assert.equal(count(), 13);
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(path.join(root, file))).digest('hex');
fs.writeFileSync(path.join(root, 'data/exports/inventec-qct-ui-audit-2026-10-04.json'), JSON.stringify({
  database_sha256: hash('data/devices.sqlite'), html_sha256: hash('data/exports/catalog.html'),
  generated_data_identities: data.length,
  verified_counts: {inventec_manufacturer: 44, inventec_all_associations: 78,
    horsea_manufacturer: 1, nx_h1331_manufacturer: 0, nx_h1331_all: 1,
    quanta_manufacturer: 41, quanta_all_associations: 73,
    quanta_historical_subsidiary_portfolio: 9,
    bytedance_all_associations: 0, inspur_all_associations: 13},
  method: 'Actual generated payload with inline script and DOM test harness; no browser screenshot',
}, null, 2) + '\n');
