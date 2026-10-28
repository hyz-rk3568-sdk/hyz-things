export function createPwaController({ current, latest, flush, reload, render }) {
    const state = { update: false, busy: false, error: '' };
    let checking = false;
    return {
        async check() {
            if (checking || !current) return;
            checking = true;
            try {
                const version = await latest();
                if (version && version !== current) {
                    state.update = true;
                    render(state);
                }
            } catch (_) {
                // Offline checks leave the current exercise untouched.
            } finally {
                checking = false;
            }
        },
        async refresh() {
            if (state.busy) return;
            state.busy = true;
            state.error = '';
            render(state);
            try {
                await flush();
                reload();
            } catch (_) {
                state.error = '草稿保存失败，已停止刷新。请保留当前页面并重试。';
            } finally {
                state.busy = false;
                render(state);
            }
        },
    };
}

async function readLatestVersion() {
    const response = await fetch('/?frontend-version-check=1', { cache: 'no-store', credentials: 'same-origin' });
    if (!response.ok) return '';
    const html = new DOMParser().parseFromString(await response.text(), 'text/html');
    const version = html.querySelector('meta[name="hyz-frontend-version"]')?.content || '';
    return /^[a-f0-9]{64}$/.test(version) ? version : '';
}

export function mountPwa(root, flush) {
    const manual = root.querySelector('[data-pwa-reload]');
    const update = root.querySelector('[data-pwa-update]');
    const status = root.querySelector('[data-pwa-status]');
    const controller = createPwaController({
        current: document.querySelector('meta[name="hyz-frontend-version"]')?.content || '',
        latest: readLatestVersion,
        flush,
        reload: () => window.location.reload(),
        render: state => {
            manual.disabled = state.busy;
            update.disabled = state.busy;
            update.hidden = !state.update;
            status.textContent = state.error || (state.busy ? '正在保存草稿…' : state.update ? '发现新版本，可在方便时点击更新。' : '');
        },
    });
    const refresh = () => void controller.refresh();
    const foreground = () => { if (document.visibilityState === 'visible') void controller.check(); };
    const restored = () => void controller.check();
    manual.addEventListener('click', refresh);
    update.addEventListener('click', refresh);
    document.addEventListener('visibilitychange', foreground);
    window.addEventListener('pageshow', restored);
    void controller.check();
    return () => {
        manual.removeEventListener('click', refresh);
        update.removeEventListener('click', refresh);
        document.removeEventListener('visibilitychange', foreground);
        window.removeEventListener('pageshow', restored);
    };
}
