# openNook landing

Vite + Tailwind CSS v4 (PostCSS) source for the GitHub Pages landing.

```
cd site
npm ci
npm run dev      # local preview
npm run build    # writes compiled site into ../docs
```

`vite.config.js` sets `build.outDir` to `../docs` so Pages can keep serving `/docs` with no Node deploy step. Commit the built `docs/` output after each source change.

`public/` holds static assets copied through to `docs/` (hero art, v2 stills, `.nojekyll`).
