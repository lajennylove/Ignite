import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';
import laravel from 'laravel-vite-plugin';

// The panel's assets are built into `dist/build`, which the crate embeds. The
// `dist` directory is the Laravel plugin's public directory, so `dist/hot` is the
// dev server's file while `npm run dev` runs. The build output is committed, so an
// app needs no Node.
export default defineConfig({
    plugins: [
        tailwindcss(),
        laravel({
            publicDirectory: 'dist',
            buildDirectory: 'build',
            input: ['resources/css/app.css', 'resources/js/app.js'],
            // The panel's views are read at run time, so a view edit reloads the page.
            refresh: ['../views/**/*.propel'],
        }),
    ],
});
