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
