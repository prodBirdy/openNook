// Messages: what arrives in the Messages app. Closed, the island shows how many messages are
// unread (hover for who and what); open, the recent conversations, and one tapped open with its
// last messages (photos, videos, voice messages, files and link previews) and a field to reply in.
// Both lists scroll.
//
// It reads ~/Library/Messages/chat.db (and the Contacts databases, for names) with sqlite3, so
// openNook needs Full Disk Access. Replies go through the Messages app (Automation). Photos are
// shrunk once with sips into ~/Library/Caches/openNook/messages; video frames and voice
// messages' waveforms come from helper.swift, built there with swiftc on first use.
// props: { preview: true (show message text and attachments; false shows only who), peek: false
//          (open the island briefly when a message arrives), conversations: 30, thread: 40,
//          linkPreviews: true (fetch a link's page for its title and picture, as a browser would) }

import { View, row, col, div, text, icon, image, spacer, InputState, Input } from "nook/ui";
import { exec, home, run, modDir, readTextFile } from "nook";

const POLL_MS = 2000;
const CONTACTS_MS = 30 * 60_000;
// A new message shows beside the camera this long even once read elsewhere.
const FLASH_MS = 8000;
const APPLE_EPOCH = 978307200;
const FDA_PANE = "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";
// Link previews ask for the page as Safari would: some sites send nothing to an unknown client.
const BROWSER_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
const MAX_LINKS = 200;
const PHOTO_W = 220;
const WAVE_BARS = 30;
const PHOTO_MAX_H = 240;

const sqlString = (s) => `'${String(s).replace(/'/g, "''")}'`;
// chat.db dates: nanoseconds since 2001 (seconds on old databases).
const unixSeconds = (col) => `(CASE WHEN ${col} > 100000000000 THEN ${col} / 1000000000 ELSE ${col} END + ${APPLE_EPOCH})`;

// Columns every message query reads.
const MESSAGE = `
  m.ROWID AS id, ${unixSeconds("m.date")} AS at, m.is_from_me AS mine, m.is_read AS read,
  m.text AS text, CASE WHEN m.text IS NULL OR m.text = '' THEN hex(m.attributedBody) END AS body,
  m.cache_has_attachments AS attachments, h.id AS sender,
  m.error AS failed, m.is_sent AS sent, m.is_delivered AS delivered, m.service AS service,
  c.ROWID AS chat, c.guid AS guid, c.display_name AS chatName, c.chat_identifier AS chatId, c.style AS style`;
const FROM = `
  FROM message m
  LEFT JOIN handle h ON h.ROWID = m.handle_id
  JOIN chat_message_join cmj ON cmj.message_id = m.ROWID
  JOIN chat c ON c.ROWID = cmj.chat_id`;
// Real messages: no tapbacks, edits-as-messages, group renames.
const REAL = `m.associated_message_type = 0 AND m.item_type = 0`;

function hexBytes(hex) {
  const out = new Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out;
}

function utf8(bytes) {
  try {
    return decodeURIComponent(bytes.map((b) => "%" + b.toString(16).padStart(2, "0")).join(""));
  } catch {
    return String.fromCharCode(...bytes.filter((b) => b < 128));
  }
}

// The text inside an `attributedBody` (an NSAttributedString in Apple's typedstream): after
// "NSString", five bytes of class info, then a length (one byte, or 0x81 + two bytes, or
// 0x82 + four, little-endian), then UTF-8.
function bodyText(hex) {
  if (!hex) return null;
  const marker = "4E53537472696E67";
  const at = hex.indexOf(marker);
  if (at < 0 || at % 2) return null;
  const b = hexBytes(hex.slice(at + marker.length, at + marker.length + 20000));
  let i = 5;
  let len = b[i++];
  if (len === 0x81) { len = b[i] | (b[i + 1] << 8); i += 2; }
  else if (len === 0x82) { len = b[i] | (b[i + 1] << 8) | (b[i + 2] << 16) | (b[i + 3] << 24); i += 4; }
  return utf8(b.slice(i, i + len));
}

function messageText(m) {
  return (m.text || bodyText(m.body) || "").replace(/￼/g, "").trim();
}

// Senders that take no replies: names ("Facebook", "Apple") rather than a number or an address.
// Short codes ("6700") take some (STOP, HELP), so they keep the field.
function canReply(c) {
  if (!c || c.style === 43) return true;
  const handle = c.chatId || c.sender || "";
  return handle.includes("@") || /^[+\d\s()-]+$/.test(handle);
}

// An outgoing message on its way: neither delivered nor failed, and recent.
const pending = (m) => m.mine && !m.failed && !m.delivered && Date.now() / 1000 - m.at < 600;

function kindOf(mime, path) {
  const m = mime || "";
  if (m.startsWith("image/") || /\.(heic|heif|jpe?g|png|gif|webp|tiff?)$/i.test(path || "")) return "photo";
  if (m.startsWith("video/") || /\.(mov|mp4|m4v)$/i.test(path || "")) return "video";
  if (m.startsWith("audio/") || /\.(caf|m4a|mp3|amr|aac)$/i.test(path || "")) return "audio";
  return "file";
}

const KIND_LABEL = { photo: "Photo", video: "Video", audio: "Voice message", file: "Attachment" };

function bytes(n) {
  if (!n) return "";
  if (n < 1024) return `${n} B`;
  if (n < 1 << 20) return `${Math.round(n / 1024)} KB`;
  return `${(n / (1 << 20)).toFixed(1)} MB`;
}

// `ref` against the page at `base` (QuickJS has no URL class): absolute, protocol-relative,
// root-relative or relative to the page's folder.
function resolveUrl(ref, base) {
  const origin = base.match(/^https?:\/\/[^/?#]+/i)?.[0] ?? "";
  if (/^https?:\/\//i.test(ref)) return ref;
  if (ref.startsWith("//")) return (base.match(/^https?:/i)?.[0] ?? "https:") + ref;
  if (ref.startsWith("/")) return origin + ref;
  const folder = base.replace(/[?#].*$/, "").replace(/\/[^/]*$/, "/");
  return /^https?:\/\/[^/]+$/i.test(base) ? `${origin}/${ref}` : folder + ref;
}

const hostOf = (url) => url.match(/^https?:\/\/([^/?#:]+)/i)?.[1]?.replace(/^www\./, "") ?? "";

const URL_RE = /https?:\/\/[^\s<>"']+[^\s<>"'.,;:!?)\]]/i;

function decodeEntities(s) {
  return s.replace(/&(#x[0-9a-f]+|#\d+|amp|lt|gt|quot|apos|#39|nbsp);/gi, (all, e) => {
    const l = e.toLowerCase();
    if (l[0] === "#") return String.fromCodePoint(l[1] === "x" ? parseInt(l.slice(2), 16) : parseInt(l.slice(1), 10));
    return { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " " }[l] ?? all;
  }).trim();
}

// A page's Open Graph card (og:title, og:image, …), falling back to <title>.
function linkCard(html, url) {
  const head = html.slice(0, 400_000);
  const meta = {};
  for (const tag of head.match(/<meta\b[^>]*>/gi) ?? []) {
    const key = tag.match(/\b(?:property|name)\s*=\s*["']([^"']+)["']/i)?.[1]?.toLowerCase();
    const value = tag.match(/\bcontent\s*=\s*["']([^"']*)["']/i)?.[1];
    if (key && value && !(key in meta)) meta[key] = decodeEntities(value);
  }
  const title = meta["og:title"] || meta["twitter:title"] || decodeEntities(head.match(/<title[^>]*>([^<]*)<\/title>/i)?.[1] ?? "");
  let picture = meta["og:image"] || meta["og:image:url"] || meta["twitter:image"] || null;
  if (picture) picture = resolveUrl(picture, url);
  if (picture && !/^https?:/.test(picture)) picture = null;
  if (!title && !picture) return null;
  return { title, site: meta["og:site_name"] || hostOf(url), picture };
}

// Phone numbers match on their last nine digits (+49 151… and 0151… are the same person).
function contactKey(handle) {
  if (!handle) return "";
  if (handle.includes("@")) return handle.trim().toLowerCase();
  const digits = handle.replace(/\D/g, "");
  return digits.slice(-9);
}

function ago(at) {
  const s = Math.max(0, Date.now() / 1000 - at);
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  const d = new Date(at * 1000);
  if (s < 7 * 86400) return ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][d.getDay()];
  return `${d.getDate()}.${d.getMonth() + 1}.`;
}

const AVATAR_COLORS = ["#ff9f0a", "#30d158", "#0a84ff", "#bf5af2", "#ff375f", "#64d2ff", "#ffd60a", "#ac8e68"];
function avatar(name, size, id) {
  const words = name.split(/\s+/).filter((w) => /^\p{L}/u.test(w));
  const initials = (words.length ? words.slice(0, 2).map((w) => w[0]).join("") : "#").toUpperCase();
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  const c = AVATAR_COLORS[h % AVATAR_COLORS.length];
  return div().id(id).size(size).flex_none().rounded_full().items_center().justify_center().gradient(c, c + "aa")
    .child(text(initials).text_size(size * 0.4).font_semibold().text_color("#ffffff"));
}

async function sql(db, query) {
  const { code, stdout, stderr } = await exec("sqlite3", ["-readonly", "-json", db, query]);
  if (code !== 0) throw new Error(stderr.trim() || `sqlite3 exited ${code}`);
  return stdout.trim() ? JSON.parse(stdout) : [];
}

export default class Messages extends View {
  init(props, cx) {
    this.preview = props.preview !== false;
    this.peek = props.peek === true;
    this.maxChats = Math.max(1, Number(props.conversations) || 30);
    this.maxThread = Math.max(1, Number(props.thread) || 40);
    this.linkPreviews = props.linkPreviews !== false;
    this.cache = `${home()}/Library/Caches/openNook/messages`;
    // Attachment id → { src, w, h } once shrunk, "busy" while it is, "failed".
    this.thumbs = {};
    this.thumbQueue = [];
    // URL → { title, site, picture }, or null when the page has no card; kept across reloads.
    this.links = JSON.parse(localStorage.getItem("links") ?? "{}");
    this.linksBusy = new Set();
    // Voice messages: attachment id → { duration, peaks }, kept across reloads; the one playing
    // ({ id, startedAt, duration }) and the timer moving its bars.
    this.waves = JSON.parse(localStorage.getItem("waves") ?? "{}");
    this.wavesBusy = new Set();
    this.unsent = null;
    this.playing = null;
    this.stopTick = null;
    this.helper = null;
    this.db = `${home()}/Library/Messages/chat.db`;
    this.names = JSON.parse(localStorage.getItem("names") ?? "{}");
    this.chats = [];
    this.thread = null; // { guid, messages }
    this.open = null;   // guid of the conversation shown
    this.top = null;    // newest message id seen
    this.flash = null;  // { until, message } for a message that just arrived
    this.error = null;
    this.denied = false;
    this.busy = false;
    this.sending = false;

    this.reply = InputState.new({ placeholder: "Reply" });
    this.reply.on("submit", (_, cx) => this.send(cx));
    this.reply.on("focus", (_, cx) => cx.notify());
    this.reply.on("blur", (_, cx) => cx.notify());

    cx.spawn((cx) => this.loadContacts(cx));
    cx.timer.every(CONTACTS_MS, (cx) => cx.spawn((cx) => this.loadContacts(cx)));
    cx.spawn((cx) => this.poll(cx));
    cx.timer.every(POLL_MS, (cx) => cx.spawn((cx) => this.poll(cx)));
    // Relative times ("4m") and the flash ending.
    cx.timer.every(30_000, (cx) => cx.notify());
  }

  // Names by handle, from every Contacts database (iCloud, On My Mac, …).
  async loadContacts(cx) {
    try {
      const base = `${home()}/Library/Application Support/AddressBook`;
      const { stdout } = await exec("find", [base, "-name", "AddressBook-v22.abcddb", "-maxdepth", "3"]);
      const names = {};
      const person = `TRIM(COALESCE(r.ZFIRSTNAME, '') || ' ' || COALESCE(r.ZLASTNAME, ''))`;
      for (const file of stdout.split("\n").filter(Boolean)) {
        const rows = await sql(file, `
          SELECT ${person} AS name, r.ZORGANIZATION AS org, p.ZFULLNUMBER AS handle
            FROM ZABCDRECORD r JOIN ZABCDPHONENUMBER p ON p.ZOWNER = r.Z_PK
          UNION ALL
          SELECT ${person}, r.ZORGANIZATION, e.ZADDRESS
            FROM ZABCDRECORD r JOIN ZABCDEMAILADDRESS e ON e.ZOWNER = r.Z_PK`).catch(() => []);
        for (const r of rows) {
          const key = contactKey(r.handle);
          const name = r.name || r.org;
          if (key && name && !names[key]) names[key] = name;
        }
      }
      if (Object.keys(names).length) {
        this.names = names;
        localStorage.setItem("names", JSON.stringify(names));
        cx.notify();
      }
    } catch (e) {
      console.warn(`contacts: ${e.message}`);
    }
  }

  name(handle) {
    if (!handle) return "Unknown";
    return this.names[contactKey(handle)] ?? handle;
  }

  chatName(c) {
    if (c.chatName) return c.chatName;
    if (c.style === 43) return "Group";
    return this.name(c.chatId || c.sender);
  }

  async poll(cx) {
    if (this.busy) return;
    this.busy = true;
    try {
      const [{ top } = {}] = await sql(this.db, `SELECT max(ROWID) AS top FROM message`);
      this.denied = false;
      this.error = null;
      if (top !== this.top) {
        const first = this.top === null;
        this.top = top;
        await this.loadChats();
        const newest = this.chats.map((c) => c.last).sort((a, b) => b.id - a.id)[0];
        if (!first && newest && !newest.mine) {
          this.flash = { until: Date.now() + FLASH_MS, message: newest };
          cx.timer.after(FLASH_MS + 50, (cx) => cx.notify());
          if (this.peek) run("island.peek", { mod: "messages", ms: 5000 });
        }
        if (this.open) await this.loadThread(this.open, cx);
      } else if (this.open && this.thread?.guid === this.open && this.thread.messages.some(pending)) {
        // Delivery changes a row, not the newest id.
        await this.loadThread(this.open, cx);
      }
    } catch (e) {
      const msg = String(e.message ?? e);
      this.denied = /authorization denied|unable to open|not permitted/i.test(msg);
      this.error = this.denied ? null : msg.split("\n").pop();
    } finally {
      this.busy = false;
    }
    cx.notify();
  }

  async loadChats() {
    const week = `(strftime('%s','now') - ${APPLE_EPOCH} - 7 * 86400) * 1000000000`;
    const rows = await sql(this.db, `
      SELECT ${MESSAGE},
        (SELECT a.mime_type FROM message_attachment_join aj JOIN attachment a ON a.ROWID = aj.attachment_id
          WHERE aj.message_id = m.ROWID LIMIT 1) AS mime,
        (SELECT count(*) FROM chat_message_join j JOIN message u ON u.ROWID = j.message_id
          WHERE j.chat_id = c.ROWID AND u.is_from_me = 0 AND u.is_read = 0
            AND u.associated_message_type = 0 AND u.item_type = 0 AND u.date > ${week}) AS unread
      ${FROM}
      WHERE m.ROWID IN (
        SELECT max(j.message_id) FROM chat_message_join j JOIN message mm ON mm.ROWID = j.message_id
        WHERE mm.associated_message_type = 0 AND mm.item_type = 0 GROUP BY j.chat_id)
      ORDER BY m.date DESC LIMIT ${this.maxChats + 4}`);
    this.chats = rows.map((r) => {
      const label = messageText(r) || (r.attachments ? KIND_LABEL[kindOf(r.mime)] : "");
      return { guid: r.guid, chatName: r.chatName, chatId: r.chatId, style: r.style, sender: r.sender, unread: r.unread, last: { ...r, body: undefined, text: label } };
    });
  }

  async loadThread(guid, cx) {
    const rows = await sql(this.db, `SELECT ${MESSAGE} ${FROM} WHERE c.guid = ${sqlString(guid)} AND ${REAL} ORDER BY m.date DESC LIMIT ${this.maxThread}`);
    const messages = rows.reverse().map((r) => ({ ...r, body: undefined, text: messageText(r), files: [] }));
    const withFiles = messages.filter((m) => m.attachments).map((m) => m.id);
    if (withFiles.length) {
      const files = await sql(this.db, `
        SELECT aj.message_id AS message, a.ROWID AS id, a.filename AS path, a.mime_type AS mime,
          a.transfer_name AS name, a.total_bytes AS size, a.is_sticker AS sticker
        FROM message_attachment_join aj JOIN attachment a ON a.ROWID = aj.attachment_id
        WHERE aj.message_id IN (${withFiles.join(",")})`);
      for (const f of files) {
        // A link preview's own payload, which the card stands in for.
        if (/pluginPayloadAttachment$/i.test(f.path || f.name || "") || !f.path) continue;
        const path = f.path.replace(/^~\//, `${home()}/`);
        messages.find((m) => m.id === f.message)?.files.push({ ...f, path, kind: kindOf(f.mime, f.path) });
      }
    }
    this.thread = { guid, messages };
    if (!cx) return;
    for (const m of messages) {
      for (const f of m.files) {
        if (f.kind === "photo" || f.kind === "video") this.wantThumb(f, cx);
        if (f.kind === "audio") this.wantWave(f, cx);
      }
      const url = this.linkPreviews && this.preview ? m.text.match(URL_RE)?.[0] : null;
      if (url) this.wantLink(url, cx);
    }
  }

  // Photos and videos are shown from a small copy, made once: one at a time, newest wanted first.
  wantThumb(f, cx) {
    if (this.thumbs[f.id]) return;
    this.thumbs[f.id] = "busy";
    this.thumbQueue.push(f);
    if (this.thumbQueue.length === 1) cx.spawn((cx) => this.drainThumbs(cx));
  }

  async drainThumbs(cx) {
    while (this.thumbQueue.length) {
      const f = this.thumbQueue[0];
      this.thumbs[f.id] = await this.makeThumb(f).catch((e) => { console.warn(`thumbnail ${f.path}: ${e.message}`); return "failed"; });
      this.thumbQueue.shift();
      cx.notify();
    }
  }

  async makeThumb(f) {
    // By id and path: ids restart in a new chat.db (and differ between Macs), paths don't.
    let hash = 0;
    for (const ch of f.path) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
    const dir = `${this.cache}/${f.id}-${hash.toString(36)}`;
    let out = `${dir}/thumb.jpg`;
    const size = async (file) => {
      const { stdout } = await exec("sips", ["-g", "pixelWidth", "-g", "pixelHeight", file]);
      const w = Number(stdout.match(/pixelWidth: (\d+)/)?.[1]), h = Number(stdout.match(/pixelHeight: (\d+)/)?.[1]);
      return w && h ? { src: file, w, h } : null;
    };
    const ready = await size(out).catch(() => null);
    if (ready) return ready;
    await exec("mkdir", ["-p", dir]);
    if (f.kind === "photo") {
      const r = await exec("sips", ["-s", "format", "jpeg", "-Z", "640", f.path, "--out", out]);
      if (r.code !== 0) throw new Error(r.stderr.trim() || "sips failed");
    } else {
      const r = await exec(await this.ensureHelper(), ["thumb", f.path, out]);
      if (r.code !== 0) throw new Error(r.stderr.trim() || "helper failed");
    }
    return (await size(out)) ?? "failed";
  }

  // helper.swift, built once (and again when it changes); its path.
  ensureHelper() {
    this.helper ??= (async () => {
      const src = `${modDir()}/helper.swift`, bin = `${this.cache}/helper`;
      const fresh = await exec("/bin/sh", ["-c", '[ -x "$2" ] && [ "$2" -nt "$1" ]', "sh", src, bin]);
      if (fresh.code === 0) return bin;
      await exec("mkdir", ["-p", this.cache]);
      const r = await exec("xcrun", ["swiftc", "-O", src, "-o", bin]);
      if (r.code !== 0) {
        this.helper = null;
        throw new Error(`building helper.swift: ${r.stderr.split("\n").find((l) => l.includes("error")) ?? r.stderr.trim()}`);
      }
      return bin;
    })();
    return this.helper;
  }

  wantWave(f, cx) {
    if (this.waves[f.id] || this.wavesBusy.has(f.id)) return;
    this.wavesBusy.add(f.id);
    cx.spawn(async (cx) => {
      try {
        const r = await exec(await this.ensureHelper(), ["wave", f.path, String(WAVE_BARS)]);
        if (r.code !== 0) throw new Error(r.stderr.trim());
        this.waves[f.id] = JSON.parse(r.stdout);
        const keys = Object.keys(this.waves);
        for (const k of keys.slice(0, Math.max(0, keys.length - 200))) delete this.waves[k];
        localStorage.setItem("waves", JSON.stringify(this.waves));
      } catch (e) {
        console.warn(`waveform ${f.path}: ${e.message}`);
        this.waves[f.id] = { duration: 0, peaks: [] };
      }
      this.wavesBusy.delete(f.id);
      cx.notify();
    });
  }

  wantLink(url, cx) {
    if (url in this.links || this.linksBusy.has(url)) return;
    this.linksBusy.add(url);
    cx.spawn(async (cx) => {
      let card = null;
      try {
        const res = await fetch(url, { headers: { "User-Agent": BROWSER_UA, Accept: "text/html", "Accept-Language": "en,de;q=0.8" } });
        if (res.ok) card = linkCard(await res.text(), url);
      } catch (e) {
        console.warn(`link preview ${url}: ${e.message}`);
      }
      this.linksBusy.delete(url);
      this.links[url] = card;
      const keys = Object.keys(this.links);
      for (const k of keys.slice(0, Math.max(0, keys.length - MAX_LINKS))) delete this.links[k];
      localStorage.setItem("links", JSON.stringify(this.links));
      cx.notify();
    });
  }

  // Plays a voice message, or stops it when it's the one playing (afplay can't resume).
  play(cx, f) {
    const again = this.playing?.id === f.id;
    this.stopPlaying(cx);
    if (again) return;
    const duration = this.waves[f.id]?.duration || 0;
    const playing = { id: f.id, startedAt: Date.now(), duration };
    this.playing = playing;
    this.stopTick = cx.timer.every(100, (cx) => cx.notify());
    cx.notify();
    cx.spawn(async (cx) => {
      // The pid goes to a file so a second tap can stop it.
      await exec("/bin/sh", ["-c", 'afplay "$1" & echo $! > "$2"; wait', "sh", f.path, `${this.cache}/afplay.pid`]).catch(() => {});
      if (this.playing === playing) this.stopPlaying(cx);
    });
  }

  stopPlaying(cx) {
    if (!this.playing) return;
    this.playing = null;
    this.stopTick?.();
    this.stopTick = null;
    cx.spawn(async () => {
      const pid = (await readTextFile(`${this.cache}/afplay.pid`))?.trim();
      if (/^\d+$/.test(pid ?? "")) await exec("kill", [pid]).catch(() => {});
    });
    cx.notify();
  }

  show(cx, guid) {
    this.open = guid;
    this.thread = this.thread?.guid === guid ? this.thread : null;
    this.reply.set_value("");
    cx.notify();
    cx.spawn(async (cx) => {
      await this.loadThread(guid, cx).catch((e) => { this.error = String(e.message ?? e); });
      cx.notify();
    });
  }

  back(cx) {
    this.open = null;
    this.thread = null;
    this.reply.blur();
    cx.notify();
  }

  send(cx) {
    const textToSend = this.reply.value().trim();
    const guid = this.open;
    if (!textToSend || !guid || this.sending) return;
    this.sending = true;
    this.reply.set_value("");
    cx.notify();
    cx.spawn(async (cx) => {
      const attempt = (args) => exec("osascript", args).catch((e) => ({ code: -1, stdout: "", stderr: String(e.message ?? e) }));
      // AppleScript's own form first; JavaScript's spelling of the same as a second try.
      const applescript = ["-e", "on run argv", "-e", 'tell application "Messages" to send (item 2 of argv) to chat id (item 1 of argv)', "-e", "end run", guid, textToSend];
      const jxa = ["-l", "JavaScript", "-e", 'function run(a) { const M = Application("Messages"); M.send(a[1], { to: M.chats.byId(a[0]) }); }', guid, textToSend];
      let r = await attempt(applescript);
      if (r.code !== 0) {
        console.warn(`send (AppleScript) to ${guid}: ${r.stderr.trim()}`);
        const second = await attempt(jxa);
        if (second.code !== 0) console.warn(`send (JavaScript) to ${guid}: ${second.stderr.trim()}`);
        else r = second;
      }
      this.sending = false;
      if (r.code !== 0) {
        const why = r.stderr.trim().split("\n").pop().replace(/^.*execution error: /, "");
        this.error = /not allowed|-1743/i.test(r.stderr)
          ? "openNook may not control Messages: allow it in System Settings › Privacy & Security › Automation."
          : `Not sent: ${why}`;
        this.unsent = { guid, text: textToSend };
        if (!this.reply.value()) this.reply.set_value(textToSend);
      } else {
        console.log(`sent to ${guid}`);
        this.error = null;
        this.unsent = null;
      }
      cx.notify();
      await this.poll(cx);
    });
  }

  unread() {
    return this.chats.reduce((n, c) => n + (c.unread || 0), 0);
  }

  activity() {
    const unread = this.unread();
    const flash = this.flash && Date.now() < this.flash.until ? this.flash.message : null;
    if (!unread && !flash) return null;
    const latest = flash ?? this.chats.find((c) => c.unread)?.last;
    const who = latest ? this.name(latest.sender) : "";
    const line = latest ? (this.preview ? `${who}: ${latest.text}` : who) : "";
    return {
      leading: icon("message-circle", 15).color("green").when(flash, (i) => i.pulse(1.2)),
      trailing: text(String(unread || 1)).id("unread").text_size(13).font_semibold().tabular_nums().text_color("green"),
      detail: text(line).text_size(12).font_medium().truncate(),
      priority: flash ? 60 : 30,
    };
  }

  expanded() {
    const page = col().id("messages").w_full().gap(8).px(4).py(6);
    if (this.denied) return page.child(this.denial());
    if (this.open) return page.child(this.threadView());

    const chats = this.chats.slice(0, this.maxChats);
    page.child(row().id("head").w_full().gap(8).px(4)
      .child(text("Messages").text_size(17).font_bold())
      .when(this.unread(), (r) => r.child(text(`${this.unread()} unread`).text_size(13).font_medium().text_color("green")))
      .child(spacer())
      .child(this.roundButton("open-app", image("app:com.apple.MobileSMS").size(16), () => exec("open", ["-a", "Messages"]))));
    if (!chats.length) {
      page.child(text(this.error ?? (this.top === null ? "Reading Messages…" : "No conversations")).id("empty").px(4).py(10).text_size(13).text_color("muted"));
      return page;
    }
    page.child(col().id("chats").w_full().max_h(290).gap(2).overflow_y_scroll().children(chats.map((c) => this.chatRow(c))));
    if (this.error) page.child(text(this.error).id("error").px(4).text_size(11).line_clamp(2).text_color("orange"));
    return page;
  }

  chatRow(c) {
    const name = this.chatName(c);
    const last = c.last;
    const line = last.mine ? `You: ${last.text}` : c.style === 43 ? `${this.name(last.sender).split(" ")[0]}: ${last.text}` : last.text;
    return row().id(`chat-${c.guid}`).w_full().gap(10).px(6).py(6).rounded(12).hover_bg("surface")
      .on_click((_, cx) => this.show(cx, c.guid))
      .child(avatar(name, 34, `av-${c.guid}`))
      .child(col().flex_1().gap(2)
        .child(row().w_full().gap(6)
          .child(text(name).flex_1().text_size(13).font_semibold().truncate())
          .child(text(ago(last.at)).text_size(11).tabular_nums().text_color("faint")))
        .child(row().w_full().gap(6)
          .child(text(this.preview ? line : c.unread ? "New message" : "").flex_1().text_size(12).truncate().text_color(c.unread ? "fg" : "muted"))
          .when(c.unread, (r) => r.child(div().h(16).px(5).rounded_full().bg("blue").items_center().justify_center()
            .child(text(String(c.unread)).text_size(10).font_bold().text_color("#ffffff"))))));
  }

  threadView() {
    const c = this.chats.find((x) => x.guid === this.open);
    const name = c ? this.chatName(c) : "";
    const group = c?.style === 43;
    const messages = this.thread?.guid === this.open ? this.thread.messages : [];
    const head = row().id("thread-head").w_full().gap(8)
      .child(this.roundButton("back", icon("chevron-left", 15).color("fg"), (cx) => this.back(cx)))
      .child(avatar(name || "?", 26, "thread-av"))
      .child(text(name).flex_1().text_size(15).font_semibold().truncate());

    // As Messages does: how the last message of ours fared, under it.
    const last = [...messages].reverse().find((m) => m.mine);
    const shown = messages.map((m) => (m === last && !m.failed
      ? { ...m, status: m.delivered ? "Delivered" : m.sent ? "Sent" : pending(m) ? "Sending…" : "Not sent" }
      : m));
    const bubbles = col().id(`thread-${this.open}`).w_full().max_h(230).gap(4).stick_bottom()
      .children(shown.map((m) => this.message(m, group)));

    const typing = this.reply.is_focused();
    const composer = !canReply(c)
      ? row().id("reply").w_full().gap(8).px(12).py(8).rounded_full().bg("surface")
        .child(icon("info", 13).color("muted"))
        .child(text(`${name} doesn't take replies`).flex_1().text_size(12).text_color("muted"))
      : row().id("reply").w_full().gap(8).pl(12).pr(4).py(4).rounded_full()
      .bg("surface").border(1, typing ? "blue" : "border")
      .child(Input.new(this.reply).id("reply-field").flex_1().text_size(13))
      .child(div().id("send").size(26).flex_none().rounded_full().items_center().justify_center()
        .bg(this.reply.value().trim() || this.sending ? "blue" : "raised")
        .on_click((_, cx) => this.send(cx))
        .child(icon(this.sending ? "ellipsis" : "send", 13).color("#ffffff")));

    return col().id("thread").w_full().gap(10)
      .child(head)
      .child(messages.length ? bubbles : text("Loading…").id("thread-loading").text_size(12).text_color("muted"))
      .child(composer)
      .when(this.error, (p) => p.child(row().id("error").w_full().gap(8)
        .child(text(this.error).flex_1().text_size(11).line_clamp(2).text_color("orange"))
        .when(this.unsent?.guid === this.open, (r) => r.child(row().id("draft").flex_none().px(10).py(4).rounded_full().bg("surface").hover_bg("raised")
          .on_click(() => this.openDraft(this.unsent))
          .child(text("Open in Messages").text_size(11).font_semibold())))));
  }

  // Messages with the text as a draft, for a send AppleScript refused: a one-to-one chat by its
  // number or address, else just the app.
  openDraft({ guid, text: body }) {
    const handle = guid.includes(";-;") ? guid.split(";-;").pop() : null;
    if (!handle) return exec("open", ["-a", "Messages"]);
    run("open", { target: `imessage://${encodeURIComponent(handle)}?body=${encodeURIComponent(body)}` });
  }

  // One message: its attachments, then its text (a link alone shows only its card), then the
  // card of the first link in it.
  message(m, group) {
    const side = (el) => row().w_full().when(m.mine, (r) => r.justify_end()).child(el);
    const parts = [];
    if (group && !m.mine) parts.push(text(this.name(m.sender).split(" ")[0]).text_size(10).font_semibold().text_color("muted").pl(11));
    if (!this.preview) {
      parts.push(side(this.bubble(m, "Message")));
    } else {
      for (const f of m.files) parts.push(side(this.attachment(f, m.mine)));
      const url = this.linkPreviews ? m.text.match(URL_RE)?.[0] : null;
      const card = url ? this.links[url] : null;
      const textOnly = card && m.text.trim() === url ? "" : m.text;
      if (textOnly) parts.push(side(this.bubble(m, textOnly)));
      if (card) parts.push(side(this.linkCardView(url, card, m.mine)));
    }
    if (m.mine && m.failed) {
      parts.push(row().gap(4).pr(4).child(icon("triangle-alert", 11).color("red"))
        .child(text(m.service === "SMS" ? "Not Delivered · SMS from a Mac needs Text Message Forwarding on your iPhone" : "Not Delivered").text_size(10).line_clamp(2).text_color("red")));
    } else if (m.status) {
      parts.push(text(m.status).pr(4).text_size(10).font_medium().text_color("muted"));
    }
    return col().id(`msg-${m.id}`).w_full().gap(3).when(m.mine, (c) => c.items_end()).enter("fade").children(parts);
  }

  bubble(m, body) {
    return col().max_w(330).px(11).py(6).rounded(16).bg(m.mine ? "blue" : "raised")
      .child(text(body).text_size(13).line_clamp(8).text_color(m.mine ? "#ffffff" : "fg"));
  }

  attachment(f, mine) {
    const open = () => run("open", { target: f.path });
    const thumb = this.thumbs[f.id];
    if ((f.kind === "photo" || f.kind === "video") && thumb && typeof thumb === "object") {
      let w = Math.min(PHOTO_W, thumb.w), h = (w * thumb.h) / thumb.w;
      if (h > PHOTO_MAX_H) { w = (w * PHOTO_MAX_H) / h; h = PHOTO_MAX_H; }
      if (f.sticker) { w *= 0.5; h *= 0.5; }
      return div().id(`file-${f.id}`).w(w).h(h).rounded(f.sticker ? 0 : 14).clip().on_click(open)
        .child(image(thumb.src).absolute_fill())
        .when(f.kind === "video", (d) => d.child(div().absolute_fill().items_center().justify_center()
          .child(div().size(34).rounded_full().bg("#000000aa").items_center().justify_center().child(icon("play-fill", 15).color("#ffffff")))));
    }
    if ((f.kind === "photo" || f.kind === "video") && thumb === "busy") {
      return div().id(`file-${f.id}`).w(PHOTO_W).h(140).rounded(14).bg("raised").items_center().justify_center()
        .child(icon(f.kind === "video" ? "video" : "eye", 18).color("faint").pulse(1.4));
    }
    if (f.kind === "audio") return this.voice(f, mine);
    const glyph = f.kind === "video" ? "video" : "files";
    const label = f.name || f.path.split("/").pop();
    return row().id(`file-${f.id}`).max_w(280).gap(8).pl(8).pr(12).py(6).rounded(14).bg(mine ? "#0a84ff" : "raised").hover_bg(mine ? "#3395ff" : "surface")
      .on_click(open)
      .child(div().size(28).flex_none().rounded_full().bg("#ffffff22").items_center().justify_center()
        .child(icon(glyph, 13).color("#ffffff")))
      .child(col().flex_1().gap(1)
        .child(text(label).text_size(12).font_medium().truncate().text_color(mine ? "#ffffff" : "fg"))
        .when(f.size, (c) => c.child(text(bytes(f.size)).text_size(10).text_color(mine ? "#ffffffaa" : "muted"))));
  }

  // A voice message as Messages draws one: play, the waveform (bars light up as it plays) and
  // its length (the time so far while it plays).
  voice(f, mine) {
    const wave = this.waves[f.id];
    const playing = this.playing?.id === f.id ? this.playing : null;
    const duration = wave?.duration || 0;
    const elapsed = playing ? Math.min(duration, (Date.now() - playing.startedAt) / 1000) : 0;
    const progress = playing && duration ? elapsed / duration : 0;
    const peaks = wave?.peaks?.length ? wave.peaks : Array(WAVE_BARS).fill(0.15);
    const lit = mine ? "#ffffff" : "fg";
    const dim = mine ? "#ffffff66" : "#ffffff40";
    const clock = (t) => `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, "0")}`;
    const bars = row().id(`wave-${f.id}`).h(26).gap(2).items_center().children(peaks.map((p, i) =>
      div().id(`bar-${f.id}-${i}`).w(2.5).h(Math.max(3, Math.round(3 + p * 21))).rounded_full()
        .bg(playing && (i + 0.5) / peaks.length <= progress ? lit : (playing ? dim : (mine ? "#ffffffcc" : "#ffffff99")))));
    return row().id(`file-${f.id}`).gap(10).pl(10).pr(14).py(7).rounded(20).bg(mine ? "blue" : "raised")
      .on_click((_, cx) => this.play(cx, f))
      .child(div().size(22).flex_none().items_center().justify_center()
        .child(icon(playing ? "pause-fill" : "play-fill", 16).color(lit)))
      .child(bars)
      .child(text(wave ? clock(playing ? elapsed : duration) : "–:––").w(30).text_size(12).font_medium().tabular_nums().text_right()
        .text_color(mine ? "#ffffffcc" : "muted"));
  }

  linkCardView(url, card, mine) {
    return col().id(`link-${url}`).w(260).rounded(14).clip().bg(mine ? "#0a84ff" : "raised").hover_bg(mine ? "#3395ff" : "surface")
      .on_click(() => run("open", { target: url }))
      .when(card.picture, (c) => c.child(image(card.picture).w(260).h(136)))
      .child(col().gap(2).px(10).py(7)
        .when(card.title, (c) => c.child(text(card.title).text_size(12).font_semibold().line_clamp(2).text_color(mine ? "#ffffff" : "fg")))
        .child(text(card.site).text_size(11).truncate().text_color(mine ? "#ffffffaa" : "muted")));
  }

  roundButton(id, glyph, onClick) {
    return div().id(id).size(26).flex_none().rounded_full().items_center().justify_center().bg("surface").hover_bg("raised")
      .on_click((_, cx) => onClick(cx))
      .child(glyph);
  }

  denial() {
    return col().id("denied").w_full().items_start().gap(10).px(4).py(8)
      .child(row().gap(8)
        .child(icon("lock", 16).color("orange"))
        .child(text("openNook needs Full Disk Access to read Messages").flex_1().text_size(13).font_semibold().line_clamp(2)))
      .child(text("Messages keeps your conversations in a protected database. Turn on openNook in Full Disk Access, then quit and reopen openNook.")
        .text_size(12).line_clamp(3).text_color("muted"))
      .child(row().id("open-settings").gap(6).px(12).py(6).rounded_full().bg("surface").hover_bg("raised")
        .on_click(() => run("open", { target: FDA_PANE }))
        .child(icon("settings", 12).color("fg"))
        .child(text("Open Full Disk Access").text_size(12).font_semibold()));
  }
}
