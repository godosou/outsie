// The product's name, for anything that can import TypeScript.
//
// Derived from brand.json so there is one place to edit. The declaration sites
// that cannot import — tauri.conf.json, package.json, the static HTML shells,
// the Rust and Kotlin sources — are checked against the same file by
// brandAssets.test.ts, which names each one. Renaming the product is: edit
// brand.json, run that test, fix what it lists.
//
// This exists because the last rename touched forty files by hand, and the one
// thing that kept it honest was a grep. A grep finds what you thought to look
// for.

import brand from '../../brand.json'

/** "Outsie" — how the product refers to itself in prose. */
export const BRAND_NAME: string = brand.name

/** "outsie." — the lowercase wordmark, with its full stop. */
export const BRAND_WORDMARK: string = brand.wordmark

/** "歇一会" — the line that follows the name in window titles. */
export const BRAND_TAGLINE: string = brand.tagline
