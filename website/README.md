# Outsie homepage

**[Visit the homepage](https://godosou.github.io/outsie/)** · [Application source](../)

AI 时代，先照顾好自己。别让 AI 把你榨干。

A bilingual product homepage with break, stretching, Phone Key, and phone workspace demos. Chinese is the default language; the language switch changes the copy to English. The demos run locally in the browser and do not connect to real devices, record audio, send messages, or execute AI tasks. No analytics, mailing-list service, or external font requests are included.

## Development

Requires Node.js 22.13 or newer.

```sh
cd website
npm ci
npm run dev
```

## GitHub Pages

From the `website/` directory:

```sh
NEXT_PUBLIC_BASE_PATH=/outsie SITE_URL=https://godosou.github.io/outsie/ npm run build:pages
```

The build generates static HTML, JavaScript, CSS, fonts, and images in `dist/pages/`. Asset URLs are checked before deployment. The workflow in [`.github/workflows/pages.yml`](../.github/workflows/pages.yml) publishes this directory when homepage changes are pushed to `main`. It uses GitHub's built-in workflow token and requires no repository secrets.

The page is built with React, vinext, and Tailwind CSS. Flower artwork comes from the app. Manrope is self-hosted with its license in `public/fonts/LICENSE`. The stretching preview uses the app's CC0 MakeHuman scene; its source attribution is in [`src/assets/STRETCH-HUMAN-LICENSE.md`](../src/assets/STRETCH-HUMAN-LICENSE.md).
