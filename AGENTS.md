# Outsie project instructions

## Releases must update the homepage

Every new public application release must include a corresponding homepage update in the same release task. This is an explicit owner requirement. Do not consider a release complete when only the GitHub Release and installation files are published.

- Update `website/lib/release.ts` with the version, release notes, download assets and checksum links.
- Update the homepage's supported platforms, installation guidance, included features and demos to match the shipped version. Keep Chinese and English content consistent and distinguish browser simulations from native functionality available in the download.
- Keep README download references and release notes consistent with the homepage.
- Run the homepage's TypeScript, lint and GitHub Pages static build checks. After publication, confirm the Pages deployment succeeds and the public homepage links to the new downloadable assets.
- Preserve the owner's requirement to remove sensitive information from public source, assets and release artifacts.

## Public publication checkout

The sanitized GitHub publication checkout is `.public/outsie/`; its `main` branch publishes to `godosou/outsie`. The original repository and worktrees retain separate development history. Transfer only intended reviewed changes into the publication checkout; do not push original unsanitized history or private hosting configuration. See `docs/plans/2026-09-08-github-publication.md` for details.
