#!/usr/bin/env node
// Builds the mod gallery into docs/gallery: an index of every mod, one static page per mod (so an
// agent that fetches its URL reads the files to add), and gallery.json (the same, as a feed).
//
// Entries come from public GitHub repos with the topic `opennook-mod`, and the mods openNook
// ships (crates/nook/mods) as examples. In a repo, a mod is any folder holding both `mod.json`
// and `main.js`; one repo may hold several. Beside it: `preview.png` (an `opennook shot`),
// `RECIPE.md` (a brief another agent could rebuild it from), both optional.
//
// No dependencies: Node 20+ and its fetch. Run from anywhere:
//   GITHUB_TOKEN=$(gh auth token) node site/gallery/build.mjs
// GALLERY_REPOS=owner/repo,owner/other adds repos as if they had the topic (for trying it out).

import { execFileSync } from "node:child_process";
import { cp, mkdir, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const HERE = path.join(ROOT, "site/gallery");
const OUT = path.join(ROOT, "docs/gallery");
const SITE = "https://prodbirdy.github.io/openNook";
const REPO = "prodBirdy/openNook";
const TOPIC = "opennook-mod";
const TOKEN = process.env.GITHUB_TOKEN;
const SKIP = (seg) => seg.startsWith(".") || seg === "node_modules";

// ---------------------------------------------------------------- GitHub

async function gh(route) {
  const res = await fetch(`https://api.github.com${route}`, {
    headers: {
      accept: "application/vnd.github+json",
      "user-agent": "opennook-gallery",
      "x-github-api-version": "2022-11-28",
      ...(TOKEN ? { authorization: `Bearer ${TOKEN}` } : {}),
    },
  });
  if (!res.ok) throw new Error(`${route}: ${res.status} ${(await res.text()).slice(0, 200)}`);
  return res.json();
}

const raw = (full, sha, file) => `https://raw.githubusercontent.com/${full}/${sha}/${file.split("/").map(encodeURIComponent).join("/")}`;

// Every public repo with the topic. A failed search fails the build, so a hiccup never deploys
// a gallery without its community mods.
async function topicRepos() {
  const repos = [];
  for (let page = 1; page <= 10; page++) {
    const r = await gh(`/search/repositories?q=${encodeURIComponent(`topic:${TOPIC} fork:true`)}&sort=updated&per_page=100&page=${page}`);
    repos.push(...r.items);
    if (r.items.length < 100) break;
  }
  for (const full of (process.env.GALLERY_REPOS ?? "").split(",").map((s) => s.trim()).filter(Boolean)) {
    if (!repos.some((r) => r.full_name.toLowerCase() === full.toLowerCase())) repos.push(await gh(`/repos/${full}`));
  }
  return repos;
}

// The mods in one repo, at its default branch's head.
async function repoMods(repo) {
  const full = repo.full_name;
  const branch = await gh(`/repos/${full}/branches/${encodeURIComponent(repo.default_branch)}`);
  const sha = branch.commit.sha;
  const tree = await gh(`/repos/${full}/git/trees/${sha}?recursive=1`);
  if (tree.truncated) console.warn(`${full}: tree truncated; mods deep in it may be missed`);
  const blobs = tree.tree.filter((t) => t.type === "blob" && !t.path.split("/").some(SKIP)).map((t) => t.path);
  const has = new Set(blobs);
  const dirOf = (p) => (p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : "");
  const join = (dir, f) => (dir ? `${dir}/${f}` : f);
  const dirs = [...new Set(blobs.filter((p) => p.endsWith("mod.json")).map(dirOf))].filter((d) => has.has(join(d, "main.js")) && has.has(join(d, "mod.json")));
  const single = dirs.length === 1;
  const owner = repo.owner.login;
  const used = new Set();

  const mods = [];
  for (const dir of dirs) {
    let manifest = {};
    try {
      const res = await fetch(raw(full, sha, join(dir, "mod.json")));
      manifest = res.ok ? JSON.parse(await res.text()) : {};
    } catch (e) {
      console.warn(`${full}/${dir}: mod.json: ${e.message}`);
    }
    const name = dir ? dir.split("/").pop() : repo.name;
    let slug = single ? "" : slugify(name);
    for (let i = 2; used.has(slug); i++) slug = `${slugify(name)}-${i}`;
    used.add(slug);
    // Beside the mod first, then beside its mods/ folder (a `<pack>/mods/<name>/` layout), then
    // the repo's root when the repo is this one mod.
    const pack = /(^|\/)mods\/[^/]+$/.test(dir) ? dir.replace(/\/?mods\/[^/]+$/, "") : null;
    const find = (f) => [join(dir, f), pack != null ? join(pack, f) : null, single ? f : null].find((p) => p && has.has(p));
    const iconFile = typeof manifest.icon === "string" && manifest.icon.startsWith("./") ? path.posix.normalize(join(dir, manifest.icon.slice(2))) : null;
    const icon = iconFile && /\.png$/i.test(iconFile) && has.has(iconFile) ? raw(full, sha, iconFile) : null;
    const preview = find("preview.png");
    const recipe = find("RECIPE.md");
    // A mod at the repo's root is the whole repo.
    const files = dir ? blobs.filter((p) => p.startsWith(`${dir}/`)) : blobs;
    mods.push({
      id: `${owner}/${repo.name}${slug ? `/${slug}` : ""}`.toLowerCase(),
      kind: "community",
      name,
      title: str(manifest.title) || name,
      description: str(manifest.description) || str(repo.description) || "",
      owner,
      avatar: repo.owner.avatar_url,
      repo: full,
      repoUrl: repo.html_url,
      folderUrl: `${repo.html_url}/tree/${encodeURIComponent(repo.default_branch)}/${dir}`,
      stars: repo.stargazers_count ?? 0,
      updated: branch.commit.commit?.committer?.date ?? repo.pushed_at,
      license: repo.license?.spdx_id && repo.license.spdx_id !== "NOASSERTION" ? repo.license.spdx_id : null,
      preview: preview ? raw(full, sha, preview) : null,
      icon,
      recipe: recipe ? `${repo.html_url}/blob/${sha}/${recipe}` : null,
      files: files.map((p) => ({ path: dir ? p.slice(dir.length + 1) : p, raw: raw(full, sha, p) })),
    });
  }
  return mods;
}

// ---------------------------------------------------------------- shipped mods

async function exampleMods() {
  const base = path.join(ROOT, "crates/nook/mods");
  const ref = process.env.GITHUB_SHA || "main";
  const mods = [];
  for (const name of (await readdir(base)).sort()) {
    const dir = path.join(base, name);
    const files = await walk(dir);
    if (!files.includes("main.js") || !files.includes("mod.json")) continue;
    const manifest = JSON.parse(await readFile(path.join(dir, "mod.json"), "utf8"));
    const rel = `crates/nook/mods/${name}`;
    const screen = path.join(ROOT, "assets/screens", `${name}.png`);
    let preview = null;
    if (await exists(screen)) {
      await cp(screen, path.join(OUT, "previews", `${name}.png`));
      preview = `previews/${name}.png`;
    }
    let icon = null;
    if (files.includes("icon.png")) {
      await cp(path.join(dir, "icon.png"), path.join(OUT, "icons", `${name}.png`));
      icon = `icons/${name}.png`;
    }
    mods.push({
      id: `prodbirdy/opennook/${name}`,
      kind: "example",
      name,
      title: str(manifest.title) || name,
      description: str(manifest.description) || "",
      owner: "prodBirdy",
      avatar: "https://github.com/prodBirdy.png?size=64",
      repo: REPO,
      repoUrl: `https://github.com/${REPO}`,
      folderUrl: `https://github.com/${REPO}/tree/main/${rel}`,
      stars: null,
      updated: lastChange(rel),
      license: "MIT",
      preview,
      icon,
      recipe: null,
      files: files.map((p) => ({ path: p, raw: raw(REPO, ref, `${rel}/${p}`) })),
    });
  }
  return mods;
}

function lastChange(rel) {
  try {
    return execFileSync("git", ["log", "-1", "--format=%cI", "--", rel], { cwd: ROOT, encoding: "utf8" }).trim() || null;
  } catch {
    return null;
  }
}

async function walk(dir, prefix = "") {
  const out = [];
  for (const e of (await readdir(dir, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
    if (SKIP(e.name)) continue;
    if (e.isDirectory()) out.push(...(await walk(path.join(dir, e.name), `${prefix}${e.name}/`)));
    else out.push(`${prefix}${e.name}`);
  }
  return out;
}

const exists = (p) => stat(p).then(() => true, () => false);
const str = (v) => (typeof v === "string" ? v.trim() : "");
const slugify = (s) => s.toLowerCase().replace(/[^a-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "") || "mod";

// ---------------------------------------------------------------- HTML

const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

function day(iso) {
  if (!iso) return "";
  const d = new Date(iso);
  const opts = { month: "short", day: "numeric", timeZone: "UTC" };
  if (d.getUTCFullYear() !== new Date().getUTCFullYear()) opts.year = "numeric";
  return d.toLocaleDateString("en-US", opts);
}

// `up` is the way back to docs/gallery from the page being written.
const asset = (url, up) => (url && !/^https?:/.test(url) ? up + url : url);
const pageUrl = (m) => `${SITE}/gallery/${m.id}/`;
const tellAgent = (m) => `Read ~/.config/opennook/AGENTS.md, then add this mod: ${pageUrl(m)}`;

function shot(m, up) {
  if (m.preview) return `<div class="shot"><img src="${esc(asset(m.preview, up))}" alt="" loading="lazy"></div>`;
  if (m.icon) return `<div class="shot icon"><img src="${esc(asset(m.icon, up))}" alt="" loading="lazy"></div>`;
  return `<div class="shot"><span class="letter">${esc(m.title.slice(0, 1).toUpperCase())}</span></div>`;
}

function layout({ title, description, up, body }) {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>${esc(title)}</title>
<meta name="description" content="${esc(description)}">
<meta property="og:title" content="${esc(title)}">
<meta property="og:description" content="${esc(description)}">
<link rel="icon" href="${up}../linux-03-compact.png" type="image/png">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=Roboto+Mono:wght@400;500&display=swap" rel="stylesheet">
<link rel="stylesheet" href="${up}gallery.css">
</head>
<body>
<header class="top"><div class="top-in">
<a class="brand" href="${up}../">openNook</a>
<nav class="nav"><a href="${up}../#widgets">Widgets</a><a href="${up}" aria-current="page">Gallery</a><a href="https://github.com/${REPO}/releases">Changelog</a></nav>
<a class="pill" href="https://github.com/${REPO}/releases">Download</a>
</div></header>
<main>
${body}
</main>
<footer><span>openNook · Open source, MIT</span><a href="https://github.com/${REPO}">GitHub</a></footer>
<script>
for (const b of document.querySelectorAll("[data-copy]")) b.addEventListener("click", async () => {
  await navigator.clipboard.writeText(b.dataset.copy);
  const was = b.textContent; b.textContent = "Copied"; setTimeout(() => (b.textContent = was), 1400);
});
</script>
</body>
</html>
`;
}

const SHARE_PROMPT = `Read ~/.config/opennook/AGENTS.md, then share my <name> mod the way its "Mods from the gallery" section describes: write a RECIPE.md beside it (a brief another agent could rebuild it from with an empty folder), render a preview.png with opennook shot, put the mod's folder, the recipe and the preview in a public GitHub repo and add the topic ${TOPIC}.`;

function card(m) {
  const search = [m.title, m.name, m.owner, m.description].join(" ").toLowerCase();
  const right = m.kind === "example" ? `<span class="tag">Example</span>` : `<span class="stars">★ ${m.stars}</span>`;
  return `<a class="card" href="${esc(m.id)}/" data-kind="${m.kind}" data-text="${esc(search)}" data-stars="${m.stars ?? -1}" data-updated="${esc(m.updated ?? "")}">
${shot(m, "")}
<div class="meta"><img class="av" src="${esc(m.avatar)}" alt="" loading="lazy"><div class="who"><b>${esc(m.title)}</b><span>${esc(m.owner)}</span></div>${right}</div>
<p class="blurb">${esc(m.description)}</p>
</a>`;
}

function indexPage(mods) {
  const community = mods.filter((m) => m.kind === "community").length;
  const body = `
<section class="hero">
<h1>Mods</h1>
<p>Everything openNook shows is a mod: a folder of JavaScript your agent can write in a minute. These are the ones people made and shared.</p>
<div class="count">${mods.length} mod${mods.length === 1 ? "" : "s"} · ${community} from the community</div>
</section>
<div class="bar">
<div class="seg" role="group" aria-label="Show">
<button type="button" data-filter="all" aria-pressed="true">All</button>
<button type="button" data-filter="example" aria-pressed="false">Examples</button>
<button type="button" data-filter="community" aria-pressed="false">Community</button>
</div>
<input class="search" type="search" placeholder="Search mods" aria-label="Search mods">
<select class="sort" aria-label="Sort">
<option value="updated">Recently updated</option>
<option value="stars">Most stars</option>
</select>
</div>
<div class="grid">
${mods.map(card).join("\n")}
</div>
<p class="none" hidden>No mods match.</p>

<section class="share" id="share">
<h2>Share yours</h2>
<p class="lead">Made a mod you like? Three steps, no sign up. It shows up here within the hour, under Community.</p>
<div class="steps">
<div class="step"><b>1. Ask your agent to write it up</b><span>A <code>RECIPE.md</code> beside the mod: the brief another agent could build it from with an empty folder.</span></div>
<div class="step"><b>2. Take a picture</b><span><code>opennook shot preview.png --mod &lt;name&gt;</code> renders its page. Put it beside the mod as <code>preview.png</code>.</span></div>
<div class="step"><b>3. Push it</b><span>A public GitHub repo with the topic <code>${TOPIC}</code>. Each mod is a folder with <code>mod.json</code> and <code>main.js</code>; one repo can hold several.</span></div>
</div>
<div class="say"><p><span class="label">Or paste this to your agent</span>${esc(SHARE_PROMPT)}</p><button class="pill" type="button" data-copy="${esc(SHARE_PROMPT)}">Copy</button></div>
<p class="note">Mods aren't reviewed by openNook. They're code with your permissions: your agent reads every file before it adds one.</p>
</section>
<script>
(() => {
  const grid = document.querySelector(".grid"), cards = [...grid.children], none = document.querySelector(".none");
  const search = document.querySelector(".search"), sort = document.querySelector(".sort"), seg = document.querySelectorAll("[data-filter]");
  let kind = "all";
  const apply = () => {
    const q = search.value.trim().toLowerCase();
    let shown = 0;
    for (const c of cards) { c.hidden = !((kind === "all" || c.dataset.kind === kind) && (!q || c.dataset.text.includes(q))); shown += !c.hidden; }
    none.hidden = shown > 0;
    const by = sort.value;
    cards.sort((a, b) => by === "stars" ? b.dataset.stars - a.dataset.stars || b.dataset.updated.localeCompare(a.dataset.updated) : b.dataset.updated.localeCompare(a.dataset.updated)).forEach((c) => grid.append(c));
  };
  seg.forEach((b) => b.addEventListener("click", () => { kind = b.dataset.filter; seg.forEach((x) => x.setAttribute("aria-pressed", x === b)); apply(); }));
  search.addEventListener("input", apply);
  sort.addEventListener("change", apply);
  apply();
})();
</script>`;
  return layout({ title: "Mods · openNook", description: "Mods for openNook, the island in your MacBook's notch: made by people and their agents, shared on GitHub.", up: "", body });
}

function detailPage(m) {
  const up = "../".repeat(m.id.split("/").length);
  const report = `https://github.com/${REPO}/issues/new?title=${encodeURIComponent(`Gallery report: ${m.id}`)}&body=${encodeURIComponent(`${pageUrl(m)}\n\nWhat's wrong with this mod:\n`)}`;
  const say = m.kind === "example"
    ? `<div class="say"><p><span class="label">Ships with openNook</span>It's already in ~/.config/opennook/mods/${esc(m.name)}. To make your own from it: Read ~/.config/opennook/AGENTS.md, then copy the ${esc(m.name)} mod to a new name and change it.</p></div>`
    : `<div class="say"><p><span class="label">Tell your agent</span>${esc(tellAgent(m))}</p><button class="pill" type="button" data-copy="${esc(tellAgent(m))}">Copy</button></div>`;
  const facts = [
    m.kind === "community" ? `★ ${m.stars}` : "Example",
    m.updated ? `Updated ${day(m.updated)}` : null,
    m.license,
  ].filter(Boolean).map(esc).join(" · ");
  const body = `
<a class="back" href="${up}">← All mods</a>
<div class="detail">
${shot(m, up)}
<div>
<h1>${esc(m.title)}</h1>
<div class="byline"><img class="av" src="${esc(m.avatar)}" alt=""><span><a href="https://github.com/${esc(m.owner)}">${esc(m.owner)}</a> · ${facts}</span></div>
<p class="desc">${esc(m.description)}</p>
${say}
<div class="links">
${m.recipe ? `<a class="pill white" href="${esc(m.recipe)}">Read the recipe</a>` : ""}
<a class="pill" href="${esc(m.folderUrl)}">View on GitHub</a>
</div>
<div class="files">
<h2>Files</h2>
<ul>
${m.files.map((f) => `<li><a href="${esc(f.raw)}">${esc(f.path)}</a></li>`).join("\n")}
</ul>
<details class="raws"><summary>Raw URLs</summary><pre>${m.files.map((f) => esc(f.raw)).join("\n")}</pre></details>
</div>
<p class="note">${m.kind === "example" ? "Made by openNook." : `Mods aren't reviewed by openNook. Your agent reads every file before it adds this one, and your own mods stay as they are.`} <a href="${report}">Report this mod</a></p>
</div>
</div>
<!-- For agents: the mod "${esc(m.name)}" is the files listed above; fetch each raw URL into ~/.config/opennook/mods/${esc(m.name)}/ keeping its path. -->`;
  return layout({ title: `${m.title} · openNook mods`, description: m.description || `An openNook mod by ${m.owner}.`, up, body });
}

// ---------------------------------------------------------------- main

async function main() {
  await rm(OUT, { recursive: true, force: true });
  await mkdir(path.join(OUT, "previews"), { recursive: true });
  await mkdir(path.join(OUT, "icons"), { recursive: true });

  const blocked = JSON.parse(await readFile(path.join(HERE, "blocklist.json"), "utf8")).map((s) => s.toLowerCase());
  const isBlocked = (full) => blocked.includes(full.toLowerCase()) || blocked.includes(full.split("/")[0].toLowerCase());

  const examples = await exampleMods();
  const repos = (await topicRepos()).filter((r) => !r.private && !r.disabled && !isBlocked(r.full_name) && r.full_name.toLowerCase() !== REPO.toLowerCase());
  console.log(`${repos.length} repo(s) with topic ${TOPIC}`);

  const community = [];
  // A few repos at a time: kind to the API, quick enough for a few hundred.
  const queue = [...repos];
  await Promise.all(Array.from({ length: 6 }, async () => {
    for (let repo; (repo = queue.shift()); ) {
      try {
        const mods = await repoMods(repo);
        if (!mods.length) console.warn(`${repo.full_name}: no folder with mod.json and main.js`);
        community.push(...mods);
      } catch (e) {
        console.warn(`${repo.full_name}: ${e.message}`);
      }
    }
  }));

  community.sort((a, b) => String(b.updated).localeCompare(String(a.updated)));
  const mods = [...community, ...examples];

  await cp(path.join(HERE, "gallery.css"), path.join(OUT, "gallery.css"));
  await writeFile(path.join(OUT, "index.html"), indexPage(mods));
  for (const m of mods) {
    await mkdir(path.join(OUT, m.id), { recursive: true });
    await writeFile(path.join(OUT, m.id, "index.html"), detailPage(m));
  }
  const feed = mods.map((m) => ({ ...m, page: pageUrl(m), preview: m.preview && !/^https?:/.test(m.preview) ? `${SITE}/gallery/${m.preview}` : m.preview, icon: m.icon && !/^https?:/.test(m.icon) ? `${SITE}/gallery/${m.icon}` : m.icon }));
  await writeFile(path.join(OUT, "gallery.json"), JSON.stringify({ generated: new Date().toISOString(), topic: TOPIC, mods: feed }, null, 2) + "\n");
  console.log(`gallery: ${examples.length} example(s), ${community.length} community mod(s) → ${path.relative(ROOT, OUT)}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
