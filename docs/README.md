# Launch page

`index.html` is the launch page: light, centered, pill CTA.

https://prodbirdy.github.io/openNook/

Primary CTA is the macOS 2.0 alpha DMG:
https://github.com/prodBirdy/openNook/releases/download/v2.0.0-alpha.1/openNook-2.0.0-alpha.1.dmg
(it 404s until that release is published).

Gallery frames are real `opennook shot` renders in `docs/screens/`. Regenerate them with
`./scripts/screens.sh` (a throwaway config home per shot; live data is whatever this Mac sees,
so play a track first for Now Playing and check the Herdr agent list before committing).

The `linux-03-*.png` frames are the 1.x GPUI Linux client and are no longer linked.

Pages: Settings → Pages → Deploy from a branch → `main` · `/docs`. No CNAME.
