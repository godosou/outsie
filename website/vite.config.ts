import tailwindcss from '@tailwindcss/postcss';
import vinext from 'vinext';
import { defineConfig } from 'vite';

export default defineConfig({
  // The homepage uses hash navigation. Only asset URLs need a Pages prefix.
  base: process.env.GITHUB_PAGES === 'true'
    ? `${process.env.NEXT_PUBLIC_BASE_PATH ?? ''}/`
    : '/',
  css: { postcss: { plugins: [tailwindcss()] } },
  plugins: [vinext()],
});
