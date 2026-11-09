# Launch page

Built output of `site/` (Vite + Tailwind v4 / PostCSS). Edit source in `site/`,
then `npm run build` from `site/` to refresh this folder.

https://prodbirdy.github.io/openNook/

- Download: https://github.com/prodBirdy/openNook/releases/download/v2.0.0-alpha.2/openNook-2.0.0-alpha.2.dmg
- Repo / Star: https://github.com/prodBirdy/openNook
- Releases: https://github.com/prodBirdy/openNook/releases

`screens/` holds the screenshots: the top of a MacBook display, the island against the
wallpaper, captured from the real app. Replace them under the same names.

`gallery/` is not committed: `site/gallery/build.mjs` generates it at deploy time from repos
with the topic `opennook-mod` (see `.github/workflows/pages.yml`).

Pages deploys `docs/` from `main` via `.github/workflows/pages.yml`. No CNAME.
The CSS in `docs/assets/` is the Tailwind build — not `cdn.tailwindcss.com`.
