// Exercise the actual generated HTML data and its inline script without navigation.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const {mount} = require('../../tests/report.test.cjs');
const root = path.resolve(__dirname, '../..');
const html = fs.readFileSync(path.join(root, 'data/exports/catalog.html'), 'utf8');
const data = JSON.parse(html.match(/<script id="data" type="application\/json">([\s\S]*?)<\/script>/)[1]);
assert.equal(data.length, 3900);
assert(!html.includes('__TARGET_NAMES__'));
const ui = mount(data);
const count = () => Number(ui.nodes.count.textContent.match(/^\d+/)[0]);
ui.set('vendor', 'Foxconn');
assert.equal(count(), 169);
ui.set('scope', 'all');
assert.equal(count(), 198);
ui.set('role', 'parent_company_portfolio');
assert.equal(count(), 8);
ui.set('query', 'SV2221A');
assert.deepEqual(ui.titles(), ['Ingrasys SV2221A']);
ui.set('scope', 'manufacturer');
assert.equal(count(), 0);
ui.set('role', ''); ui.set('query', ''); ui.set('scope', 'all');
ui.set('vendor', 'ByteDance'); assert.equal(count(), 0);
ui.set('vendor', 'Inspur'); assert.equal(count(), 13);
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(path.join(root, file))).digest('hex');
fs.writeFileSync(path.join(root, 'data/exports/ingrasys-ui-audit-2026-10-04.json'), JSON.stringify({
  database_sha256: hash('data/devices.sqlite'), html_sha256: hash('data/exports/catalog.html'),
  generated_data_identities: data.length,
  verified_counts: {foxconn_manufacturer: 169, foxconn_all_associations: 198,
    foxconn_parent_company_portfolio: 8, sv2221a_foxconn_manufacturer: 0,
    bytedance_all_associations: 0, inspur_all_associations: 13},
  method: 'Actual generated payload with inline script and DOM test harness; no browser screenshot',
}, null, 2) + '\n');
