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
    load: function() {
        return Promise.all([uci.load('bcm_switch'), callStatus()]);
    },

    render: function(data) {
        const statusText = E('p');
        const portText = E('pre', { 'style': 'white-space:pre;overflow:auto;max-height:28em' });
        const refresh = function(status) {
            statusText.textContent = !status.running ? _('ASIC service stopped') :
                !status.initialized ? _('ASIC initialization has not completed; check the SDK console log.') :
                status.applying ? _('Applying ASIC configuration…') :
                '%s — %s'.format(status.chip, status.last_apply || _('Ready'));
            statusText.style.color = status.initialized && status.last_apply_ok ? '' : '#b06000';
            if (status.ports) portText.textContent = status.ports;
        };
        refresh(data[1]);
        poll.add(function() {
            return callStatus().then(refresh).catch(function() {
                statusText.textContent = _('Unable to read ASIC status');
            });
        }, 5);

        const activeProfile = data[1].profile || '40g';
        const portCount = activeProfile === '40g' ? 16 : 64;
        const m = new form.Map('bcm_switch', _('Broadcom ASIC'),
            _('Configure BCM56845 hardware switching. Management Ethernet is separate. Settings are stored in RAM during netboot testing.'));
        let s = m.section(form.NamedSection, 'core', 'switch', _('Switch'));
        let o = s.option(form.Flag, 'enabled', _('Enable front-panel ports'));
        o.rmempty = false;
        o.default = '1';
        o = s.option(form.ListValue, 'profile', _('Lane profile'));
        o.value('40g', _('16 × 40 Gb/s'));
        o.value('10g', _('64 × 10 Gb/s breakout'));
        o.rmempty = false;
        o.description = _('Changing profile restarts the ASIC and changes logical port mapping. Reload this page after applying a profile change.');

        s = m.section(form.GridSection, 'port', _('Physical ports'));
        s.anonymous = false;
        s.addremove = false;
        s.filter = function(section) { return Number(section.slice(2)) < portCount; };
        o = s.option(form.DummyValue, '_name', _('Port'));
        o.cfgvalue = function(section) { return section; };
        o = s.option(form.Flag, 'enabled', _('Enabled'));
        o.rmempty = false;
        o = s.option(form.ListValue, 'speed', _('Speed'));
        o.value('0', _('Maximum / automatic'));
        o.value('1000', _('1 Gb/s'));
        o.value('10000', _('10 Gb/s'));
        if (activeProfile === '40g') o.value('40000', _('40 Gb/s'));
        o.rmempty = false;
        o = s.option(form.Flag, 'autoneg', _('Autonegotiation'));
        o.rmempty = false;
        o = s.option(form.Value, 'pvid', _('Untagged ingress VLAN (PVID)'));
        o.datatype = 'range(1,4094)';
        o.rmempty = false;

        s = m.section(form.GridSection, 'vlan', _('VLAN membership'));
        s.anonymous = true;
        s.addremove = true;
        o = s.option(form.Value, 'vid', _('VLAN ID'));
        o.datatype = 'range(1,4094)';
        o.rmempty = false;
        for (const kind of ['tagged', 'untagged']) {
            o = s.option(form.DynamicList, kind, kind === 'tagged' ? _('Tagged ports') : _('Untagged ports'));
            o.value('cpu', _('ASIC CPU port'));
            for (let index = 0; index < portCount; index++) o.value('xe' + index, 'xe' + index);
            o.validate = function(section, value) {
                return value === '' || /^(cpu|xe(?:[0-9]|[1-5][0-9]|6[0-3]))$/.test(value)
                    ? true : _('Choose a physical port or the ASIC CPU port');
            };
        }

        const retry = E('button', { 'class': 'cbi-button cbi-button-action',
            'click': ui.createHandlerFn(this, function() {
                return callApply().then(function(result) {
                    ui.addNotification(null, E('p', {}, result.message || _('No response')), result.ok ? 'info' : 'error');
                    return callStatus().then(refresh);
                });
            }) }, _('Reapply saved ASIC configuration'));
        return m.render().then(function(map) {
            return E('div', {}, [statusText, map, retry,
                E('details', {}, [E('summary', {}, _('Live ASIC port status')), portText])]);
        });
    }
});
