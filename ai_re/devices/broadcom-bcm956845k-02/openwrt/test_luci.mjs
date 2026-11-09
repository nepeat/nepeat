// Headless Chromium must be listening on loopback port 9223.
import fs from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.dirname(fileURLToPath(import.meta.url));
const nativeView = process.env.BCM_LUCI_NATIVE === '1';
const password = fs.readFileSync(path.join(root, 'work/luci-root-password.txt'), 'utf8').trim();
const page = await (await fetch('http://127.0.0.1:9223/json/new?about:blank', { method: 'PUT' })).json();
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
let nextId = 0;
const pending = new Map(), exceptions = [], loginExceptions = [];
let awaitingLoginNavigation = false;
socket.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.method === 'Page.frameNavigated' && !message.params.frame.parentId && awaitingLoginNavigation) {
        loginExceptions.push(...exceptions);
        exceptions.length = 0;
        awaitingLoginNavigation = false;
    }
    if (message.method === 'Runtime.exceptionThrown') exceptions.push(message.params.exceptionDetails.exception?.description || message.params.exceptionDetails.text);
    if (message.id && pending.has(message.id)) {
        const { resolve, reject } = pending.get(message.id);
        pending.delete(message.id);
        if (message.error) reject(new Error(message.error.message)); else resolve(message.result);
    }
};
socket.onclose = () => { for (const item of pending.values()) item.reject(new Error('CDP connection closed')); pending.clear(); };
function call(method, params = {}) {
    const id = ++nextId;
    return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        socket.send(JSON.stringify({ id, method, params }));
    });
}
async function evaluate(expression) {
    const response = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (response.exceptionDetails) throw new Error(response.exceptionDetails.exception?.description || response.exceptionDetails.text);
    return response.result.value;
}
const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
try {
    await call('Page.enable');
    await call('Runtime.enable');
    await call('Network.clearBrowserCookies');
    await call('Page.navigate', { url: nativeView ? 'https://10.60.2.203/cgi-bin/luci/admin/network/network' : 'https://10.60.2.203/cgi-bin/luci/admin/network/bcm_switch' });
    for (let i = 0; i < 40; i++) {
        if (await evaluate('!!document.querySelector("input[name=luci_password]") || !!document.querySelector(".cbi-map")')) break;
        await delay(500);
    }
    if (await evaluate('!!document.querySelector("input[name=luci_password]")')) {
        awaitingLoginNavigation = true;
        await evaluate(`(() => {
            document.querySelector('input[name=luci_username]').value = 'root';
            const input = document.querySelector('input[name=luci_password]');
            input.value = ${JSON.stringify(password)};
            input.form.submit();
        })()`);
    }
    for (let i = 0; i < 60; i++) {
        if (await evaluate('!!document.querySelector(".cbi-map")')) break;
        await delay(500);
    }
    let applyTest;
    if (process.env.BCM_UI_APPLY_TEST === '1') {
        const readPort = () => JSON.parse(execFileSync('bash', [path.join(root, 'ssh.sh'), 'bcmctl --json "port xe0"'], { encoding: 'utf8', timeout: 15000 })).output;
        async function toggleAndApply(disabled) {
            await evaluate(`(() => {
                const input = document.querySelector('input[type=checkbox]');
                if (!input) throw new Error('Enable checkbox missing');
                input.click();
                const button = document.querySelector('.cbi-button-apply');
                if (!button) throw new Error('Save & Apply button missing');
                button.click();
            })()`);
            for (let i = 0; i < 45; i++) {
                await delay(1000);
                if (readPort().includes('DISABLED') === disabled) return true;
            }
            throw new Error('LuCI apply did not reach expected ASIC state');
        }
        try {
            await toggleAndApply(true);
            await delay(5000);
            await toggleAndApply(false);
            applyTest = 'LuCI Save & Apply disabled and re-enabled xe0; ASIC readback verified';
        } finally {
            execFileSync('bash', [path.join(root, 'ssh.sh'), 'uci set bcm_switch.core.enabled=1; uci commit bcm_switch; /usr/libexec/bcm-switch apply'], { timeout: 60000 });
        }
    }
    const result = await evaluate(`({
        title: document.title,
        hasMap: !!document.querySelector('.cbi-map'),
        has40G: document.body.innerText.includes('16 × 40'),
        hasVLAN: document.body.innerText.includes('VLAN membership'),
        hasNativePort: document.body.innerText.includes('swp1'),
        text: document.body.innerText.slice(0, 8000)
    })`);
    result.exceptions = exceptions;
    result.loginExceptions = loginExceptions;
    if (applyTest) result.applyTest = applyTest;
    fs.writeFileSync(path.join(root, nativeView ? 'work/luci-native-browser-result.json' : 'work/luci-browser-result.json'), JSON.stringify(result, null, 2));
    const screenshot = await call('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true });
    fs.writeFileSync(path.join(root, nativeView ? 'work/luci-native.png' : 'work/luci-asic.png'), Buffer.from(screenshot.data, 'base64'));
    console.log(JSON.stringify(result));
    if (!result.hasMap || (nativeView ? !result.hasNativePort : (!result.has40G || !result.hasVLAN)) || exceptions.length) process.exitCode = 1;
} finally {
    await call('Page.close').catch(() => {});
    socket.close();
}
