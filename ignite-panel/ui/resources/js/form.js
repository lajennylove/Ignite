// The behaviour of the create and edit forms, which only a form page loads, and only when the
// form asks before it is left or has a slug field.

// Unsaved changes alerts, as Filament's `unsavedChangesAlerts()`. A form marked
// `data-ignite-unsaved-changes-alert` remembers its values when the page loads. When the user
// follows a link, or closes or reloads the page, while a form's values differ from those, the
// panel asks first. A form marked `data-ignite-unsaved` is changed already: it is shown again
// after a save that failed. Sending the form is not leaving it, so it does not ask.
const unsavedMessage = 'Are you sure you want to leave? Without saving, everything is going to be lost.';
const savedValues = new WeakMap();
let leaving = false;

// The values of a form, as text, without the panel's own fields (`_token`, `_ignite_shown`).
function formValues(form) {
    const values = [];
    for (const [name, value] of new FormData(form)) {
        if (!name.startsWith('_')) {
            values.push([name, typeof value === 'string' ? value : value.name]);
        }
    }
    return JSON.stringify(values);
}

function unsavedForm() {
    return [...document.querySelectorAll('form[data-ignite-unsaved-changes-alert]')].some(
        (form) => form.hasAttribute('data-ignite-unsaved') || savedValues.get(form) !== formValues(form),
    );
}

document.querySelectorAll('form[data-ignite-unsaved-changes-alert]').forEach((form) => {
    savedValues.set(form, formValues(form));
});

document.addEventListener('submit', (event) => {
    if (!event.defaultPrevented && event.target.matches?.('form[data-ignite-unsaved-changes-alert]')) {
        leaving = true;
    }
});

// A page restored from the browser's cache was left with its form unsent.
window.addEventListener('pageshow', () => {
    leaving = false;
});

window.addEventListener('beforeunload', (event) => {
    if (!leaving && unsavedForm()) {
        event.preventDefault();
        event.returnValue = unsavedMessage;
    }
});

document.addEventListener('click', (event) => {
    const link = event.target.closest?.('a[href]');
    if (
        !link ||
        event.defaultPrevented ||
        event.button !== 0 ||
        event.metaKey ||
        event.ctrlKey ||
        event.shiftKey ||
        event.altKey ||
        link.target === '_blank' ||
        link.hasAttribute('download') ||
        link.getAttribute('href').startsWith('#')
    ) {
        return;
    }
    if (!unsavedForm()) {
        return;
    }
    if (window.confirm(unsavedMessage)) {
        leaving = true;
    } else {
        event.preventDefault();
    }
});

// A slug field (`data-ignite-slug-from`, Filament's slug pattern) takes the slug of its source
// field as the user types, until the user edits it: when the slug no longer matches the slug
// of the source's previous value, it is left alone, as Filament's example compares it with
// `Str::slug($old)`. A slug field marked `data-ignite-slug-locked` (the record is saved with a
// slug, and `slug_locked_when` holds, such as for a published post) is never filled.
const previousValues = new WeakMap();

// The slug of `text`, as Laravel's `Str::slug`: ASCII, lowercase, words joined by `-`.
function slug(text) {
    return text
        .normalize('NFKD')
        .replace(/[̀-ͯ]/g, '')
        .replace(/@/g, ' at ')
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/^-+|-+$/g, '');
}

document.addEventListener('input', (event) => {
    const source = event.target;
    const form = source.form;
    if (!form || !source.name) {
        return;
    }
    const previous = previousValues.has(source) ? previousValues.get(source) : source.defaultValue;
    previousValues.set(source, source.value);
    for (const target of form.querySelectorAll('[data-ignite-slug-from]:not([data-ignite-slug-locked])')) {
        if (target.dataset.igniteSlugFrom === source.name && target.value === slug(previous)) {
            target.value = slug(source.value);
        }
    }
});

// The add, delete and move buttons of a repeater, group or flexible content field
// (`data-ignite-op`, as Filament's repeater actions) send the form to the form's live URL,
// which answers with the form shown again and saves nothing. Without this script the browser
// does the same with the button's `formaction`, and shows the page it answers; with it, only
// the fields are replaced, as a live field replaces them.
document.addEventListener('click', async (event) => {
    const button = event.target.closest?.('button[data-ignite-op]');
    const form = button?.form;
    if (!button || !form || event.defaultPrevented) {
        return;
    }
    event.preventDefault();
    const body = new FormData(form);
    body.set(button.name, button.value);
    const response = await fetch(button.formAction, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams(body),
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
});
