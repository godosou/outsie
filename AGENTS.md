# Outsie project instructions

## Releases must update the homepage

Every new public application release must include a corresponding homepage update in the same release task. This is an explicit owner requirement. Do not consider a release complete when only the GitHub Release and installation files are published.

- Update `website/lib/release.ts` with the version, release notes, download assets and checksum links.
- Update the homepage's supported platforms, installation guidance, included features and demos to match the shipped version. Keep Chinese and English content consistent and distinguish browser simulations from native functionality available in the download.
- Keep README download references and release notes consistent with the homepage.
- Run the homepage's TypeScript, lint and GitHub Pages static build checks. After publication, confirm the Pages deployment succeeds and the public homepage links to the new downloadable assets.
- Preserve the owner's requirement to remove sensitive information from public source, assets and release artifacts.

The product homepage lives in `website/`. Changes to that directory on `main` deploy through `.github/workflows/pages.yml`.
