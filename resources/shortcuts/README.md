# Nook Clock shortcuts

Clock.app has no public write API for a dev-signed Mac app. Control goes through
Shortcuts App Intents. Create four shortcuts with **exactly** these names, then
tap **Install…** in Settings → Widgets → Timers (or open the `.shortcut` files
in this folder after signing them on a Mac with `shortcuts sign`).

| Name | Action |
|------|--------|
| `Nook Start Timer` | Start Timer (Clock). Optional text input: duration in seconds. |
| `Nook Pause Timer` | Pause Timer |
| `Nook Resume Timer` | Resume Timer |
| `Nook Cancel Timer` | Cancel Timer |

`shortcuts list` must show those names. Until they are imported, the island
still **reads** Clock timers and the Clock section falls back to **Open Clock**
(`x-apple-clock:timer?id=`).
