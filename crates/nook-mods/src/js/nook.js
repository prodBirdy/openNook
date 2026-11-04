// The `nook` module: what a mod may ask of openNook and the machine. Reads of live data
// (media, agents, apps, shared) are tracked: when one changes, every mod that read it renders
// again, so views don't subscribe by hand.
const { call, start, subscribe } = globalThis.__nook;

const read = (topic) => call("read", { topic });

/** What's playing, or null. Updates about once a second while playing. */
export const media = () => read("media");
/** Live coding-agent sessions (Claude, Codex, OpenCode, Cursor, Gemini, …). */
export const agents = () => read("agents") ?? [];
/** Running GUI apps: [{ bundleId, name, pid }]. */
export const apps = () => read("apps") ?? [];
/** Whether an app with this bundle id is running (e.g. "anomaly.hex"). */
export const appRunning = (bundleId) => apps().some((a) => a.bundleId === bundleId);
/** The Mac's location: { latitude, longitude, name }, { error }, or null while it's found.
 *  The first read asks for permission. */
export const location = () => read("location");
/** The island: { expanded, selected, notch: { width, height, hasNotch }, screen: { width, height } }. */
export const island = () => read("island");

/** A value every mod can read; setting it re-renders every mod that read it. */
export const shared = (key) => (read("shared") ?? {})[key];
export const setShared = (key, value) => call("shared.set", { key, value });

/** This mod's props from layout.json (`mods.<name>.props`), else mod.json's `props`. */
export const props = () => call("props") ?? {};
export const home = () => call("path", { which: "home" });
export const appHome = () => call("path", { which: "app" });
export const modDir = () => call("path", { which: "mod" });
export const platform = () => call("path", { which: "platform" });
/** The theme's colors by token. */
export const theme = () => read("theme") ?? {};

/**
 * Runs an openNook command (fire and forget):
 *   island.expand, island.collapse, island.toggle, island.select {mod}, island.peek {mod?, ms?}
 *   media.toggle, media.play, media.pause, media.next, media.previous, media.seek {seconds},
 *   media.shuffle, media.repeat
 *   agents.focus {pid}, agents.reveal {cwd}
 *   open {target}   (a URL, file or folder)
 */
export const run = (command, args) => call("run", { command: String(command), args: args ?? {} });
export const open = (target) => run("open", { target: String(target) });

/** Runs a program with the user's environment. 30 s limit, 8 MiB of output. */
export const exec = (command, args = [], options = {}) => start("exec", { command, args, cwd: options.cwd ?? null });
/** Reads UTF-8 text (up to 4 MiB), or null when the file doesn't exist. `~/` is the home folder. */
export const readTextFile = (path) => start("read", { path });
/** Writes UTF-8 text, replacing the file atomically. Only under appHome(). */
export const writeTextFile = (path, text) => start("write", { path, text: String(text) });

/**
 * Follows a growing file (a log): calls `onLine(line, cx)` for every line appended after now
 * (or the last `fromEnd` lines first). Survives rotation. Returns a function that stops it.
 */
export const tail = (path, onLine, options = {}) => subscribe("tail", { path, fromEnd: options.fromEnd ?? 0 }, onLine);
/** Calls `onChange(text | null, cx)` with the file's contents now and whenever it changes. */
export const watchFile = (path, onChange) => subscribe("watch", { path }, onChange);
