# Outsie GitHub publication

The user requested one public personal repository containing the application and product homepage, with GitHub Pages hosting. Both source and homepage are authorized for public access. The user explicitly required sensitive information to be removed before upload.

## Publication source

- Public repository: https://github.com/godosou/outsie
- Product homepage: https://godosou.github.io/outsie/
- Sanitized publication checkout: `.public/outsie/`
- Homepage location in that checkout: `website/`
- Deployment workflow: `.github/workflows/pages.yml`

The original application repository and active worktrees retain their existing history. The public checkout has rewritten history that uses the owner's GitHub noreply address, generic developer paths and example infrastructure URLs. Historical TLS test private-key fixtures and device screenshots are excluded. The current homepage is added as ordinary source files in the same public repository; no nested Git repository or local Sites project identity is published.

All six committed application branches are retained in the public checkout. Worktree-only changes and unrelated untracked plans are not included. The homepage retains the previously approved content and demos. Public builds do not require the original Sites plugin or Cloudflare worker environment.

## Future updates

Every new public application release must also update the homepage in the same release task. This is an explicit owner requirement recorded in the project `AGENTS.md`. Update the version, download and checksum links in `website/lib/release.ts`, shipped feature descriptions and demos, installation/platform information, and matching README/release-note references. Verify the deployed homepage and its new download links before reporting the release complete.

Use `.public/outsie/` to commit and push public changes. Do not point the original repository or active worktrees at the public remote and push their unsanitized history. Transfer intended changes into the publication checkout, review the diff, and run the relevant checks. Changes under `website/` on public `main` automatically deploy to GitHub Pages.

For homepage changes, copy the relevant page/components/assets into the publication checkout while retaining its Pages configuration, public dependency lockfile and privacy exclusions. Do not copy `.openai/`, local environment files, `.git/`, build output or private validation artifacts.

## Checks

TruffleHog 3.94.0 is run against each branch without a depth limit, and against static deployment output. Credential verification requests are disabled to keep candidate secrets local; every detector finding is reviewed. Additional full-blob checks cover personal identity, internal domains, hosting identifiers and private-key markers. Scanner output stays in a system temporary directory, outside public source.

## Completed pre-publication review

- TruffleHog 3.94.0: `git` scans of all six branch histories, no depth limit; `filesystem` scan of `website/dist/pages`.
- Final detector results: verified 0, unknown 0, unverified 0. Active credential verification was deliberately disabled; these are detection results, not a claim that every possible secret type can be excluded.
- Additional privacy scan: 1183 historical blobs plus static artifacts; no remaining targeted personal identity, internal domain, hosting ID, private-key or absolute build-path matches. Commit author and committer identities use GitHub noreply.
- Public homepage build, TypeScript and lint passed. Dependencies updated to React 19.2.8, vinext 1.0.0-beta.9 and Vite 8.2.2; npm audit reported zero known vulnerabilities for the homepage dependency tree. This is not a full vulnerability audit of every native branch.
- GitHub secret scanning and secret-scanning push protection enabled.
- Detailed local scan records: `/tmp/outsie-audit/final/`. These reports are not part of the public repository.
- Public initial main commit: `570322c`; all six branch pushes succeeded.

## Deployment result

- GitHub Actions run `34191361856` completed successfully (build and deploy).
- Public homepage returned HTTP 200; 11 linked script, image, stylesheet and font resources were fetched successfully.
- Pages serves over enforced HTTPS and uses GitHub Actions as its build source.
- GitHub secret-scanning alerts API returned an empty list after publication.
- Local application history was not rewritten. The website's original local source repository retains a compatibility commit for static builds; the public checkout is the source for future GitHub deployments.

## GitHub Release v0.6.2 and download entry

The user requested that the existing build be uploaded to GitHub Release and linked from the homepage. The selected existing version is Repose 0.6.2 for Apple Silicon / macOS 14+.

The original DMG was not uploaded: its Mach-O executable contained 545 occurrences of the local user's build path. The same public application source was rebuilt with Rust `--remap-path-prefix` and Clang `-ffile-prefix-map`, preserving version and functionality. The original local release remains intact. The public build is in `.public/outsie/release/Repose-0.6.2-mac-arm64/`.

Validation completed before upload:

- 76 application tests passed.
- Main and break-page production entry checks passed.
- Bundle identity, version and Dock icon checks passed.
- Binary architecture: arm64. Minimum system version: macOS 14.
- Ad hoc signature verification and DMG integrity verification passed; no Developer ID signing or Apple notarization is claimed.
- Personal-path and credential-pattern scan of 45 app/frontend/site files: no findings.
- TruffleHog filesystem scan of application resources, app frontend, site artifact and release notes: no findings; candidate credential verification remained disabled.
- Homepage TypeScript, lint and static build passed. Download section, DMG link, release link and all internal anchors checked.
- Uploaded DMG size: 3997291 bytes. Local and GitHub asset SHA-256 both `71ac23bd22f8d76d9cf3fd84fe9446c5ddff65344c99c9e4054514f36255645e`.

The public Release is marked as a preview (prerelease) and points at the native source commit used for the build, `570322c0422a342dd80d1772cd42b7167a3f2a06`. Link directly to `https://github.com/godosou/outsie/releases/tag/v0.6.2`; do not use `/releases/latest`, which excludes prereleases.

Public homepage commit: `57e7dd1ebf1c1e10de3edb9e5424e1da65e59beb`. Release links are centralized in `website/lib/release.ts`. The header, hero and closing CTA lead to `#download`; this section provides the DMG link, release notes, checksum file, installation steps, platform requirements, installed name Repose and the Apple-notarization caveat. Phone Key and Phone Controls are explicitly outside this binary. The original local website repository has the same page/CSS/download-link update for future consistency.

Local private validation reports: `/tmp/outsie-release/`.

Release publication completed: v0.6.2 is public (not a draft). The unauthenticated DMG download returned HTTP 200 and matched the published SHA-256 checksum. GitHub Actions run `34192147186` completed successfully. The public homepage returned HTTP 200 with the new download section, direct asset/release links, platform information and notarization notice; the old unavailable-download wording is gone.

## Phone Key animated homepage demo

The user requested animation or an interactive demo for the “你带走手机。我们照看屏幕。” section. The old static two-row diagram is replaced by a phone-on-the-left and Mac-on-the-right scene in `website/app/phone-key-demo.tsx` and its stylesheet. Existing setup guidance, other demos, and the v0.6.2 download scope remain unchanged.

The ten-second sequence shows nearby → away/locked → returned/verifying → resumed. Work and break scenarios are selectable; the break scenario keeps counting down across lock/verification. Playback starts once when at least 30% visible, pauses outside the viewport or in a hidden tab, and supports manual pause/replay. Reduced-motion preferences prevent automatic playback and disable visual transitions. The status is exposed as a polite output for assistive technology, while the decorative device scene is hidden from it. No real device connection, authentication, microphone, or system locking is involved.

TypeScript, lint and GitHub Pages static export passed. The rendered output includes the scene controls, preserves the existing Release link, removes the old static diagram and contains no targeted personal paths, identities or hosting metadata. No browser interaction or visual QA was performed. Public and original local homepage checkouts contain the same demo update.

Phone Key demo publication completed at public commit `5f42a074dee87e97748056623c71c43138d62199`. GitHub Actions run `34192952943` succeeded. The public homepage contains the new animated scene and controls, no longer contains the static diagram, and retains the v0.6.2 download link. Linked public assets were checked over HTTP.

## v0.6.3 · layout, eye care and motion alignment

The owner approved the enlarged left-side 3D guide with instructions, countdown and controls on the right, then requested a review of the eight movements before release. This release also adds six stable short-break eye-care tips. The motion review corrected upper-trapezius arm/head pairing, backward shoulder-roll direction, unexplained raised arms during upper-back turns, neck/chest amplitude, and hand cropping during forward reach and side-bend transitions.

- Development main: `dd91543`.
- Sanitized public main and release target: `fba35837d22109af9ce16e5a3b0bb64a6e32adcb`.
- Public release: https://github.com/godosou/outsie/releases/tag/v0.6.3
- Package: Apple Silicon / macOS 14+, Repose, ad hoc signed and not Apple notarized. No change was made to `/Applications/Repose.app`.
- 84 application tests passed, including eight bone-landmark regression tests. All eight representative poses were visually reviewed. The framing test samples 32 phases across four aspect ratios per movement; this is an animation consistency check, not clinical certification.
- Tauri packaging, bundle identity/Dock icon, code-signature verification and DMG integrity checks passed. The binary uses compiler path remapping.
- Privacy-pattern scan: 73 changed source/artifact files, no findings. TruffleHog source and artifact scans: zero findings, credential verification disabled.
- Homepage TypeScript, lint and static export passed. The bilingual download section, eye-care illustration and side-by-side stretching illustration were updated.
- Pages run `34195211595` succeeded. Public homepage HTTP 200, new release/DMG/checksum links confirmed, and 11 linked resources returned HTTP 200.
- Public anonymous DMG download HTTP 200; 3,996,557 bytes; local, GitHub API and downloaded SHA-256 all match `e2f9369b96ad68a9e59d580bd8ccfedeab3070e173e142fe063e9a587aa56c1f`.
- Private validation reports: `/tmp/outsie-release-v063/`.

## Custom domain: outsie.dev

The owner purchased `outsie.dev` through Railway. It is active and uses Railway-managed DNS; the website remains hosted by GitHub Pages. Only Outsie domain records were changed.

- GitHub Pages custom domain: `outsie.dev`; repository homepage: `https://outsie.dev/`.
- Apex DNS: the four official GitHub Pages IPv4 and four IPv6 addresses. `www` is a CNAME to `godosou.github.io`; TTL is 600 seconds. No wildcard records were created.
- GitHub DNS health checks passed for both names. The initially stalled certificate request was restarted by rebinding the same domain, following GitHub's documented troubleshooting flow. The certificate was approved for `outsie.dev` and `www.outsie.dev`, and enforced HTTPS is enabled.
- Public main commit: `8a3ba293c9f6814da4f87a9c79f8780f6c33d3ff`. Both README homepage links, the default metadata URL, canonical link and Open Graph URL now use the custom domain.
- TypeScript, lint, root-path static build, asset-path validation and targeted public-artifact privacy checks passed. Public checkout is clean.
- Pages run `34195863998`, attempt 2, succeeded after HTTPS was enabled. The workflow used `SITE_URL=https://outsie.dev` and an empty asset base path. The downloaded deployment artifact and public `/index.html` have the HTTPS canonical and Open Graph URLs. The root-path CDN temporarily retained the prior HTTP canonical under its ten-minute cache; both root and index routes already served correctly over HTTPS.
- HTTPS root returned 200 with normal certificate verification. `https://www.outsie.dev/`, `http://outsie.dev/` and the previous `https://godosou.github.io/outsie/` all resolve through redirects to `https://outsie.dev/`.
- Eleven linked public resources returned 200. The homepage retained v0.6.3 Release, DMG and checksum links; all three download destinations returned 200 anonymously. No application binary or release was changed by the domain task.
- Verification script and downloaded build artifact are private temporary files under `/tmp/`, not committed to GitHub. Railway authentication data and account identifiers were not added to the public repository.
