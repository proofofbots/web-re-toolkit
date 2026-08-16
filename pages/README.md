# pages

The documentation site, built with [Starlight](https://starlight.astro.build/). Published to https://proofofbots.github.io/web-re-toolkit/ by `.github/workflows/pages.yml` on every push to `main` that touches this directory.

```bash
npm install
npm run dev
npm run build
```

Content lives in `src/content/docs`. Every file needs `title` and `description` frontmatter. A page only appears in the sidebar once it is listed in `astro.config.mjs`.

Links between pages are absolute and include the base path, for example `/web-re-toolkit/reference/protocol/`. A link without the base resolves off the site in production.
