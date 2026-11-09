import { cpSync, existsSync } from 'node:fs';
import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';
import laravel from 'laravel-vite-plugin';

// laravel-vite-plugin 3 does not copy images into the build, so `asset()` for a
// file in `resources/images` would have nothing to point at. Copy them into the
// build, next to the hashed files, so the panel serves them.
function copyStaticAssets() {
    return {
        name: 'ignite:copy-static-assets',
        apply: 'build',
        closeBundle() {
            if (existsSync('resources/images')) {
                cpSync('resources/images', 'dist/build/resources/images', { recursive: true });
            }
        },
    };
}

// The panel's assets are built into `dist/build`, which the crate embeds. The
// `dist` directory is the Laravel plugin's public directory, so `dist/hot` is the
// dev server's file while `npm run dev` runs. The build output is committed, so an
// app needs no Node.
export default defineConfig({
    // Relative URLs inside the CSS: the panel mounts under any path, so `/build/...` would miss.
    base: './',
    plugins: [
        tailwindcss(),
        laravel({
            publicDirectory: 'dist',
            buildDirectory: 'build',
            input: ['resources/css/app.css', 'resources/js/app.js'],
            // The panel's views are read at run time, so a view edit reloads the page.
            refresh: ['../views/**/*.propel'],
        }),
        copyStaticAssets(),
    ],
});
