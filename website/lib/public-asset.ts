/** Public assets need the repository prefix when hosted on GitHub Pages. */
export function publicAsset(path: string) {
  return `${process.env.NEXT_PUBLIC_BASE_PATH ?? ''}${path}`;
}
