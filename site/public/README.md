# Launch page

Built output of `site/` (Vite + Tailwind v4 / PostCSS). Edit source in `site/`,
then `npm run build` from `site/` to refresh this folder.

https://prodbirdy.github.io/openNook/

Verified download links (do not use the stale 1.0.0 paths):

- macOS 0.3.0: https://github.com/prodBirdy/openNook/releases/download/v0.3.0/openNook-0.3.0.dmg
- Linux 0.3.0 GPUI tarball: https://github.com/prodBirdy/openNook/releases/download/v0.3.0-linux/openNook-0.3.0-x86_64-unknown-linux-gnu.tar.gz
- Repo / Star: https://github.com/prodBirdy/openNook
- Releases / changelog: https://github.com/prodBirdy/openNook/releases

Hero, widget gallery (compact + expanded SoT cards), tray drop-zone, terminal PTY,
and platform islands are HTML chrome from the captain mock. `linux-03-*.png`
remain as the v2 product stills. Album art is `album-art.jpg`.

Pages deploys from `main` · `/docs` via `.github/workflows/pages.yml`. No CNAME.
The CSS in `docs/assets/` is the Tailwind build — not `cdn.tailwindcss.com`.
