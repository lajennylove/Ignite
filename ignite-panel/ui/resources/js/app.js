// The panel's small behaviour. A form with `data-confirm="..."` asks before it is
// sent, so a delete cannot happen by accident. The message is the attribute's text.
document.addEventListener('submit', (event) => {
    const message = event.target.dataset?.confirm;
    if (message && !window.confirm(message)) {
        event.preventDefault();
    }
});
