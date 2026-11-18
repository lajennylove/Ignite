// The panel's small behaviour. A form with `data-confirm="..."` asks before it is
// sent, so a delete cannot happen by accident. The message is the attribute's text.
document.addEventListener('submit', (event) => {
    const message = event.target.dataset?.confirm;
    if (message && !window.confirm(message)) {
        event.preventDefault();
    }
});

// The theme toggle saves the user's choice on the server, then switches the page. If the save
// fails, the page keeps its theme. The `data-theme` attribute of <html> is what the stylesheet reads.
const themeLabels = { light: 'Switch to dark theme', dark: 'Switch to light theme' };

function showTheme(button, theme) {
    document.documentElement.dataset.theme = theme;
    button.dataset.theme = theme;
    button.setAttribute('aria-label', themeLabels[theme]);
}

document.querySelectorAll('[data-theme-toggle]').forEach((button) => {
    showTheme(button, button.dataset.theme);
});

document.addEventListener('click', async (event) => {
    const button = event.target.closest?.('[data-theme-toggle]');
    if (!button) {
        return;
    }
    const next = button.dataset.theme === 'dark' ? 'light' : 'dark';
    const response = await fetch(button.dataset.themeUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams({ _token: button.dataset.csrf, theme: next }),
        credentials: 'same-origin',
    });
    if (response.ok) {
        showTheme(button, next);
    }
});

// A live field (`data-ignite-live`, Filament's `live()`) shows its form's fields again when its
// value changes. The form is sent to its `data-ignite-live-url`, the server runs the field
// conditions on the new values, and the fields it renders replace the old ones. A field is
// shown or hidden by the server's answer only, so the save checks the same conditions. The
// form then fires `ignite:fields`, so an app's editor or field script can enhance the new inputs.
document.addEventListener('change', async (event) => {
    const form = event.target.closest?.('[data-ignite-live]')?.closest('form[data-ignite-live-url]');
    if (!form) {
        return;
    }
    const response = await fetch(form.dataset.igniteLiveUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams(new FormData(form)),
        credentials: 'same-origin',
    });
    if (!response.ok) {
        return;
    }
    const page = new DOMParser().parseFromString(await response.text(), 'text/html');
    const fields = page.querySelector('[data-ignite-fields]');
    const current = form.querySelector('[data-ignite-fields]');
    if (!fields || !current) {
        return;
    }
    current.replaceWith(document.importNode(fields, true));
    form.dispatchEvent(new CustomEvent('ignite:fields', { bubbles: true }));
    const changed = event.target.name && form.elements.namedItem(event.target.name);
    changed?.focus?.();
});
