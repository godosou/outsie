import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const source = 'dist/client';
const target = 'dist/pages';
const prefix = `${process.env.NEXT_PUBLIC_BASE_PATH ?? ''}/`;
if (!existsSync(join(source, 'index.html'))) {
  throw new Error('Static export did not produce index.html.');
}

// Publish only page output and public assets, excluding build metadata.
rmSync(target, { recursive: true, force: true });
mkdirSync(target, { recursive: true });
for (const name of ['index.html', 'index.rsc', '404.html', '_next', ...readdirSync('public')]) {
  if (existsSync(join(source, name))) cpSync(join(source, name), join(target, name), { recursive: true });
}
writeFileSync(join(target, '.nojekyll'), '');

function checkAsset(url) {
  if (!url.startsWith('/') || url.startsWith('//')) return;
  if (!url.startsWith(prefix)) throw new Error(`Asset is missing the Pages prefix: ${url}`);
  const relative = url.slice(prefix.length).split(/[?#]/)[0];
  if (!existsSync(join(target, relative))) throw new Error(`Missing public asset: ${url}`);
}

const html = readFileSync(join(target, 'index.html'), 'utf8');
for (const [, url] of html.matchAll(/(?:src|href)="([^"]+)"/g)) checkAsset(url);
function checkCss(directory) {
  for (const file of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, file.name);
    if (file.isDirectory()) checkCss(path);
    else if (file.name.endsWith('.css')) {
      for (const [, url] of readFileSync(path, 'utf8').matchAll(/url\(["']?([^"')]+)["']?\)/g)) checkAsset(url);
    }
  }
}
checkCss(target);
console.log(`GitHub Pages output ready: ${target} (asset prefix ${prefix})`);
