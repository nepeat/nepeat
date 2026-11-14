'use strict';
'require view';
'require form';
'require uci';
'require rpc';
'require poll';
'require ui';
const callStatus = rpc.declare({ object: 'bcm_switch', method: 'status', expect: {} });
const callApply = rpc.declare({ object: 'bcm_switch', method: 'apply', expect: {} });
return view.extend({
    load: function() { return Promise.all([uci.load('bcm_switch'), callStatus()]); },
    render: function(data) {
        const statusText = E('p');
        const summary = E('p');
        const portText = E('pre', { style: 'white-space:pre;overflow:auto;max-height:28em' });
        const refresh = function(status) {
            statusText.textContent = !status.running ? _('ASIC service stopped') :
                !status.initialized ? _('ASIC initialization is in progress') :
                status.applying ? _('Applying native network configuration…') :
                status.message || _('Waiting for native networking');
            statusText.style.color = status.ok ? '' : '#b06000';
            summary.textContent = status.ok ? _('Hardware VLANs: %s · Router interfaces: %s · Hardware IPv4 routes: %s · CPU routes: %s')
                .format(status.vlan_count || 0, status.rif_count || 0, status.hardware_routes || 0, status.cpu_routes || 0) : '';
            if (status.warnings && status.warnings.length) summary.textContent += ' — ' + status.warnings.join('; ');
            if (status.ports) portText.textContent = status.ports;
        };
        refresh(data[1]);
        poll.add(function() { return callStatus().then(refresh).catch(function() {
            statusText.textContent = _('Unable to read switch hardware status');
        }); }, 5);
        const profile = data[1].profile || '40g';
        const m = new form.Map('bcm_switch', _('Switch hardware'),
            _('Configure lane profiles and PHY settings here. Configure interfaces, bridges, VLANs, addresses and routes in Network → Interfaces. Native network configuration is synchronized to the ASIC. Settings remain in RAM during netboot testing.'));
        let s = m.section(form.NamedSection, 'core', 'switch', _('Lane profile'));
        let o = s.option(form.ListValue, 'profile', _('Lane profile'));
        o.value('40g', _('16 × 40 Gb/s'));
        o.value('10g', _('64 × 10 Gb/s breakout'));
        o.rmempty = false;
        o.description = _('Changing profile restarts the ASIC and changes logical port mapping. Reload this page after applying a profile change.');
        s = m.section(form.GridSection, 'port', _('PHY settings'));
        s.anonymous = false; s.addremove = false;
        s.filter = function(section) { return Number(section.slice(2)) < (profile === '40g' ? 16 : 64); };
        o = s.option(form.DummyValue, '_name', _('Linux device'));
        o.cfgvalue = function(section) { return 'swp' + (Number(section.slice(2)) + 1); };
        o = s.option(form.ListValue, 'speed', _('Speed'));
        o.value('0', _('Profile maximum'));
        o.value('1000', _('1 Gb/s'));
        o.value('10000', _('10 Gb/s'));
        if (profile === '40g') o.value('40000', _('40 Gb/s'));
        o.rmempty = false;
        o = s.option(form.Flag, 'autoneg', _('Autonegotiation'));
        o.rmempty = false;
        const reconcile = E('button', { class: 'cbi-button cbi-button-action',
            click: ui.createHandlerFn(this, function() {
                return callApply().then(function(result) {
                    ui.addNotification(null, E('p', {}, result.message || _('No response')), result.ok ? 'info' : 'error');
                    return callStatus().then(refresh);
                });
            }) }, _('Reconcile native network configuration'));
        return m.render().then(function(map) {
            return E('div', {}, [statusText, summary, map, reconcile,
                E('details', {}, [E('summary', {}, _('Live ASIC port diagnostics')), portText])]);
        });
    }
});
