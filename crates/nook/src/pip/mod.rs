pub mod chrome;
pub mod lyrics;
pub mod observe;

pub use nook_core::settings::PipKind;
use gpui::{
    AnyElement, App, Bounds, Context, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Render, Subscription, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, div, prelude::*, px, size,
};
use nook_core::settings::{PipCorner, PipWindowState};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const PIP_MARGIN: f32 = 16.0;
const HOVER_ENTER_MS: u64 = 120;
const HOVER_EXIT_MS: u64 = 150;
const DRAG_SAMPLE_WINDOW_MS: u64 = 100;
const DRAG_POLL_MS: u64 = 16;
const RELEASE_PROJECTION_S: f32 = 0.2;
const MAX_FLICK_SPEED: f32 = 9000.0;
const SETTLE_TICK_MS: u64 = 8;

thread_local! {
    static OPEN: RefCell<HashMap<PipKind, (WindowHandle<PipView>, i64)>> = RefCell::new(HashMap::new());
}

pub struct PipView {
    pub kind: PipKind,
    pub island: WeakEntity<crate::island::Island>,
    pub hovered: bool,
    pub mini: bool,
    /// Owning island is the gallery mock — never touch settings / restore.
    gallery: bool,
    corner: PipCorner,
    keep_on_top: bool,
    content_height: f32,
    window_number: i64,
    /// Last frame we asked AppKit for. Frame setters are async (a main-queue
    /// flush outside any GPUI lease), so a read right after a set would see
    /// the stale frame; velocity samples and settle origins use this instead.
    requested_frame: Option<(f64, f64, f64, f64)>,
    hover_gen: u64,
    /// Stored so the Island observe callback is not dropped.
    #[allow(dead_code)]
    island_subscription: Option<Subscription>,
    dragging: bool,
    drag_moved: bool,
    drag_grab: Option<(f64, f64)>,
    drag_last_pointer: Option<(f32, f32)>,
    drag_gen: u64,
    drag_samples: Vec<(Instant, f32, f32)>,
    settle_gen: u64,
}

pub fn is_open(kind: PipKind) -> bool {
    OPEN.with(|open| open.borrow().contains_key(&kind))
}

pub fn open(kind: PipKind, island: WeakEntity<crate::island::Island>, cx: &mut App) {
    // The Pop Out control runs inside `Context<Island>::listener`, i.e. while
    // the Island is leased for update; reading it here (or opening a window
    // whose first render reads it) panics as a re-entrant read. Defer past
    // the current effect cycle, when the Island is no longer leased.
    cx.defer(move |cx| {
        let gallery = island
            .upgrade()
            .is_some_and(|entity| entity.read(cx).gallery_mode);
        let (corner, mini, keep_on_top) = if gallery {
            // Distinct corners so both windows are visible in gallery shots.
            let corner = match kind {
                PipKind::Lyrics => PipCorner::BottomRight,
                PipKind::Observe => PipCorner::BottomLeft,
            };
            (corner, false, true)
        } else {
            persisted_state(kind)
        };
        open_with_state(kind, island, corner, mini, keep_on_top, gallery, cx);
    });
}

pub fn restore(island: WeakEntity<crate::island::Island>, cx: &mut App) {
    // Same lease as `open`: callers may hold an Island update lock, so defer
    // past the current effect cycle before reading the Island to size windows.
    cx.defer(move |cx| {
        // Gallery never calls restore (and must not: open_island holds an Island
        // update lock when it invokes us). Skip any already-open kinds.
        for state in nook_core::settings::get_app_settings().pip_windows {
            if is_open(state.kind) {
                continue;
            }
            let mini = state.mini && state.kind == PipKind::Lyrics;
            open_with_state(
                state.kind,
                island.clone(),
                state.corner,
                mini,
                state.keep_on_top,
                false,
                cx,
            );
        }
    });
}

pub fn close(kind: PipKind, cx: &mut App) {
    // Deferred like `open`: settings off-switches call this from Island
    // context while the Island is leased, so never touch entities synchronously.
    cx.defer(move |cx| {
        let handle = OPEN.with(|open| open.borrow().get(&kind).map(|(handle, _)| *handle));
        if let Some(handle) = handle {
            handle
                .update(cx, |view, window, cx| view.close_window(window, cx))
                .ok();
        }
    });
}

impl PipView {
    pub fn keep_on_top(&self) -> bool {
        self.keep_on_top
    }

    pub fn toggle_keep_on_top(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.keep_on_top = !self.keep_on_top;
        crate::platform::style_pip_window_for(window, self.keep_on_top);
        self.persist(|state| state.keep_on_top = self.keep_on_top);
        cx.notify();
    }

    pub fn toggle_mini(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.kind != PipKind::Lyrics {
            return;
        }
        self.mini = !self.mini;
        self.persist(|state| state.mini = self.mini);
        self.apply_size(window, cx);
        cx.notify();
    }

    pub fn set_content_height(&mut self, height: f32, window: &mut Window, cx: &mut Context<Self>) {
        if (self.content_height - height).abs() < 0.5 {
            return;
        }
        self.content_height = height;
        self.apply_size(window, cx);
        cx.notify();
    }

    pub fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::platform::unregister_pip_window(self.window_number);
        OPEN.with(|open| {
            open.borrow_mut().remove(&self.kind);
        });
        if !self.gallery {
            nook_core::settings::tweak_app_settings(|settings| {
                settings.pip_windows.retain(|state| state.kind != self.kind);
            });
        }
        window.remove_window();
        let _ = cx;
    }

    pub fn back_to_notch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.kind;
        let island = self.island.clone();
        self.close_window(window, cx);
        if let Some(entity) = island.upgrade() {
            entity.update(cx, |island, cx| island.reveal_from_pip(kind, cx));
        }
    }

    fn window_size(&self, cx: &mut Context<Self>) -> (f32, f32) {
        let width = match self.kind {
            PipKind::Lyrics if self.mini => super::pip::lyrics::MINI_W,
            PipKind::Lyrics => super::pip::lyrics::LYRICS_W,
            PipKind::Observe => super::pip::observe::OBSERVE_W,
        };
        if self.kind == PipKind::Lyrics && self.mini {
            return (width, super::pip::lyrics::MINI_H);
        }
        (width, self.live_content_height(cx))
    }

    fn live_content_height(&self, cx: &mut Context<Self>) -> f32 {
        let view = &*self;
        self.island
            .upgrade()
            .map(|entity| {
                let island = entity.read(cx);
                match self.kind {
                    PipKind::Lyrics => super::pip::lyrics::content_height(view, island),
                    PipKind::Observe => super::pip::observe::content_height(view, island),
                }
            })
            .unwrap_or(self.content_height)
    }

    fn reduce_motion(&self, cx: &mut Context<Self>) -> bool {
        self.island
            .upgrade()
            .map(|entity| entity.read(cx).reduce_motion)
            .unwrap_or_else(crate::platform::reduce_motion)
    }

    /// Frame to reason about: the last requested origin/size when one is
    /// pending, else the live AppKit frame. Never read-after-set directly.
    fn current_frame(&self, window: &mut Window) -> Option<(f64, f64, f64, f64)> {
        self.requested_frame.or_else(|| crate::platform::pip_window_frame(window))
    }

    fn current_frame_by_number(&self) -> Option<(f64, f64, f64, f64)> {
        self.requested_frame
            .or_else(|| crate::platform::pip_frame_by_number(self.window_number))
    }

    fn note_requested(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.requested_frame = Some((x, y, w, h));
    }

    fn apply_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (width, height) = self.window_size(cx);
        self.content_height = height;
        let animate = !self.reduce_motion(cx);
        if let Some((x, y, old_w, old_h)) = self.current_frame(window) {
            let (nx, ny) =
                anchored_origin(self.corner, (x, y, old_w, old_h), (width as f64, height as f64));
            crate::platform::snap_pip_window_to(window, nx, ny, width as f64, height as f64, animate);
            self.note_requested(nx, ny, width as f64, height as f64);
        }
        self.persist(|_| {});
    }

    fn begin_drag(&mut self, window: &mut Window, pointer: (f32, f32), cx: &mut Context<Self>) {
        // Grabbing mid-settle stops the spring where the window is now: the
        // task exits on the next tick when its generation no longer matches.
        self.settle_gen += 1;
        self.dragging = true;
        self.drag_moved = false;
        self.drag_last_pointer = Some(pointer);
        // Screen-space grab: global cursor minus window origin, so each move
        // is an absolute placement (no accumulation, no drift). Falls back
        // to window-relative deltas when the global position or the frame
        // is unavailable (non-macOS).
        self.drag_grab = match (
            crate::platform::mouse_screen_location(),
            self.current_frame(window),
        ) {
            (Some((mx, my)), Some((x, y, _, _))) => Some((mx - x, my - y)),
            _ => None,
        };
        self.drag_samples.clear();
        self.push_drag_sample(window);
        self.start_drag_poll(cx);
    }

    fn drag_move(&mut self, window: &mut Window, pointer: (f32, f32)) {
        if !self.dragging {
            return;
        }
        // Screen-space path: absolute placement from the global cursor.
        if self.drag_apply_screen(window) {
            self.drag_last_pointer = Some(pointer);
            return;
        }
        let Some(last) = self.drag_last_pointer else {
            self.drag_last_pointer = Some(pointer);
            return;
        };
        let (dx, dy) = (pointer.0 - last.0, pointer.1 - last.1);
        self.drag_last_pointer = Some(pointer);
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let Some((x, y, w, h)) = self.current_frame(window) else {
            return;
        };
        // GPUI pointer space is top-left origin; Cocoa frames are bottom-left.
        let (nx, ny) = (x + dx as f64, y - dy as f64);
        crate::platform::move_pip_frame(window, nx, ny, w, h);
        self.note_requested(nx, ny, w, h);
        self.drag_moved = true;
        self.push_drag_sample_at(nx as f32, ny as f32);
    }

    /// Absolute screen-space placement: `origin = mouse_screen - grab`.
    /// Returns false when screen-space drag is unavailable so the caller
    /// falls back to window-relative deltas.
    fn drag_apply_screen(&mut self, window: &mut Window) -> bool {
        let Some((gx, gy)) = self.drag_grab else {
            return false;
        };
        let Some((mx, my)) = crate::platform::mouse_screen_location() else {
            return false;
        };
        let Some((_, _, w, h)) = self.current_frame(window) else {
            return false;
        };
        let (nx, ny) = (mx - gx, my - gy);
        crate::platform::move_pip_frame(window, nx, ny, w, h);
        self.note_requested(nx, ny, w, h);
        self.drag_moved = true;
        self.push_drag_sample_at(nx as f32, ny as f32);
        true
    }

    /// Poll-task counterpart of [`Self::drag_apply_screen`]: follows the
    /// cursor by windowNumber when GPUI stops delivering `on_mouse_move`
    /// outside the window during a fast drag. Runs on the main thread via
    /// `AsyncApp::update`, like the settle task.
    fn drag_apply_screen_by_number(&mut self) -> bool {
        let Some((gx, gy)) = self.drag_grab else {
            return false;
        };
        let Some((mx, my)) = crate::platform::mouse_screen_location() else {
            return false;
        };
        let Some((_, _, w, h)) = self.current_frame_by_number() else {
            return false;
        };
        let (nx, ny) = (mx - gx, my - gy);
        crate::platform::set_pip_frame_by_number(self.window_number, nx, ny, w, h);
        self.note_requested(nx, ny, w, h);
        self.drag_moved = true;
        self.push_drag_sample_at(nx as f32, ny as f32);
        true
    }

    fn push_drag_sample_at(&mut self, x: f32, y: f32) {
        let now = Instant::now();
        self.drag_samples.push((now, x, y));
        let cutoff = Duration::from_millis(DRAG_SAMPLE_WINDOW_MS + 20);
        self.drag_samples.retain(|(at, _, _)| now.duration_since(*at) < cutoff);
    }

    /// 16 ms poll while dragging: keeps following the cursor when it leaves
    /// the window mid-drag, and ends the drag + snaps when the button is
    /// released. The button check is also the reliable drag-end signal for
    /// three-finger drag lock, which holds ~0.5 s after the fingers lift.
    /// Only runs for screen-space drags (macOS); otherwise there is nothing
    /// to sample and the `on_mouse_up` handlers end the drag.
    fn start_drag_poll(&mut self, cx: &mut Context<Self>) {
        if self.drag_grab.is_none() || crate::platform::mouse_screen_location().is_none() {
            return;
        }
        self.drag_gen += 1;
        let generation = self.drag_gen;
        cx.spawn(async move |view, cx| loop {
            cx.background_executor().timer(Duration::from_millis(DRAG_POLL_MS)).await;
            let done = view
                .update(cx, |view, cx| {
                    if view.drag_gen != generation || !view.dragging {
                        return true;
                    }
                    if !crate::platform::pip_left_mouse_pressed() {
                        view.end_drag_from_poll(cx);
                        return true;
                    }
                    view.drag_apply_screen_by_number();
                    false
                })
                .unwrap_or(true);
            if done {
                break;
            }
        })
        .detach();
    }

    /// Snap after a poll-detected button release. Same target math as
    /// [`Self::end_drag`] but via windowNumber (no GPUI `Window` in the poll
    /// task). Always animates; AppKit ignores the animation under Reduce
    /// Motion.
    fn end_drag_from_poll(&mut self, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        if !self.drag_moved {
            return;
        }
        let now = Instant::now();
        let samples: Vec<(f64, f32, f32)> = self
            .drag_samples
            .iter()
            .filter(|(at, _, _)| now.duration_since(*at) < Duration::from_millis(DRAG_SAMPLE_WINDOW_MS))
            .map(|(at, x, y)| (-(now.duration_since(*at)).as_secs_f64(), *x, *y))
            .collect();
        let (vx, vy) = sampled_velocity(&samples);
        let Some((x, y, w, h)) = self.current_frame_by_number() else {
            return;
        };
        let from = (x as f32, y as f32);
        let target_point = project_release(from, (vx, vy));
        let screens: Vec<(f32, f32, f32, f32)> = crate::platform::pip_screen_frames()
            .into_iter()
            .map(|s| (s.0 as f32, s.1 as f32, s.2 as f32, s.3 as f32))
            .collect();
        let screen = if screens.is_empty() {
            crate::platform::pip_visible_frame_by_number(self.window_number)
                .map(|v| (v.0 as f32, v.1 as f32, v.2 as f32, v.3 as f32))
        } else {
            let (idx, _) = corner_for_point(target_point, &screens);
            screens.get(idx).copied()
        };
        let Some(screen) = screen else {
            return;
        };
        let (_, corner) = corner_for_point(target_point, std::slice::from_ref(&screen));
        self.corner = corner;
        self.persist(|state| state.corner = corner);
        let (tx, ty) = corner_origin(corner, (w as f32, h as f32), screen, PIP_MARGIN);
        crate::platform::snap_pip_frame_by_number(self.window_number, tx as f64, ty as f64, w, h, true);
        self.note_requested(tx as f64, ty as f64, w, h);
        cx.notify();
    }

    fn push_drag_sample(&mut self, window: &mut Window) {
        let now = Instant::now();
        // Prefer the just-requested origin: the async set lands a tick later.
        if let Some((x, y, _, _)) = self.current_frame(window) {
            self.drag_samples.push((now, x as f32, y as f32));
        }
        let cutoff = Duration::from_millis(DRAG_SAMPLE_WINDOW_MS + 20);
        self.drag_samples.retain(|(at, _, _)| now.duration_since(*at) < cutoff);
    }

    fn end_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        // Stop the poll timer: it exits on the next tick when it sees the
        // generation mismatch / dragging flag.
        self.drag_gen += 1;
        // A plain click never sampled motion: leave the window where it is.
        if !self.drag_moved {
            return;
        }
        let now = Instant::now();
        let samples: Vec<(f64, f32, f32)> = self
            .drag_samples
            .iter()
            .filter(|(at, _, _)| now.duration_since(*at) < Duration::from_millis(DRAG_SAMPLE_WINDOW_MS))
            .map(|(at, x, y)| (-(now.duration_since(*at)).as_secs_f64(), *x, *y))
            .collect();
        let (vx, vy) = sampled_velocity(&samples);
        let Some((x, y, w, h)) = self.current_frame(window) else {
            return;
        };
        let from = (x as f32, y as f32);
        let target_point = project_release(from, (vx, vy));
        let screens: Vec<(f32, f32, f32, f32)> = crate::platform::pip_screen_frames()
            .into_iter()
            .map(|s| (s.0 as f32, s.1 as f32, s.2 as f32, s.3 as f32))
            .collect();
        let screen = if screens.is_empty() {
            crate::platform::pip_visible_frame_for(window)
                .map(|v| (v.0 as f32, v.1 as f32, v.2 as f32, v.3 as f32))
        } else {
            let (idx, _) = corner_for_point(target_point, &screens);
            screens.get(idx).copied()
        };
        let Some(screen) = screen else {
            return;
        };
        let (_, corner) = corner_for_point(target_point, std::slice::from_ref(&screen));
        self.corner = corner;
        self.persist(|state| state.corner = corner);
        let (tx, ty) = corner_origin(corner, (w as f32, h as f32), screen, PIP_MARGIN);
        if self.reduce_motion(cx) {
            crate::platform::snap_pip_window_to(window, tx as f64, ty as f64, w, h, false);
            self.note_requested(tx as f64, ty as f64, w, h);
            cx.notify();
            return;
        }
        self.start_settle(from, (vx, vy), (tx, ty), (w, h), cx);
    }

    fn start_settle(
        &mut self,
        from: (f32, f32),
        velocity: (f32, f32),
        target: (f32, f32),
        size: (f64, f64),
        cx: &mut Context<Self>,
    ) {
        self.settle_gen += 1;
        let generation = self.settle_gen;
        let number = self.window_number;
        let (w, h) = size;
        cx.spawn(async move |view, cx| {
            let spring = crate::motion::Spring::new(0.38, 0.12);
            let mut x = crate::motion::SpringValue {
                value: from.0,
                velocity: velocity.0,
            };
            let mut y = crate::motion::SpringValue {
                value: from.1,
                velocity: velocity.1,
            };
            let mut last = Instant::now();
            loop {
                cx.background_executor().timer(Duration::from_millis(SETTLE_TICK_MS)).await;
                let dt = last.elapsed().as_secs_f32();
                last = Instant::now();
                let keep_x = x.step(spring, target.0, dt, crate::motion::REST_PX);
                let keep_y = y.step(spring, target.1, dt, crate::motion::REST_PX);
                let (px, py) = (x.value, y.value);
                let settled = !keep_x && !keep_y;
                let placed = view
                    .update(cx, |view, _| {
                        if view.settle_gen != generation {
                            return false;
                        }
                        crate::platform::set_pip_frame_by_number(
                            number, px as f64, py as f64, w, h,
                        );
                        view.note_requested(px as f64, py as f64, w, h);
                        true
                    })
                    .unwrap_or(false);
                if !placed || settled {
                    if settled {
                        let _ = view.update(cx, |_, cx| cx.notify());
                    }
                    return;
                }
            }
        })
        .detach();
    }

    fn snap_after_compositor_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Non-macOS path: the compositor ran a modal move inside
        // `start_window_move`, so there are no origin samples to project.
        // The button is up by the time that call returns.
        let frame = crate::platform::pip_window_frame(window);
        let visible = crate::platform::pip_visible_frame_for(window);
        let (Some(frame), Some(visible)) = (frame, visible) else {
            return;
        };
        let rect = (frame.0 as f32, frame.1 as f32, frame.2 as f32, frame.3 as f32);
        let area = (
            visible.0 as f32,
            visible.1 as f32,
            visible.2 as f32,
            visible.3 as f32,
        );
        self.corner = nearest_corner(rect, area);
        self.persist(|state| state.corner = self.corner);
        let (w, h) = (frame.2, frame.3);
        let (ox, oy) = corner_origin(self.corner, (w as f32, h as f32), area, PIP_MARGIN);
        let animate = !self.reduce_motion(cx);
        crate::platform::snap_pip_window_to(window, ox as f64, oy as f64, w, h, animate);
        self.note_requested(ox as f64, oy as f64, w, h);
    }

    fn arm_hover(&mut self, entering: bool, cx: &mut Context<Self>) {
        self.hover_gen += 1;
        let generation = self.hover_gen;
        let delay = Duration::from_millis(if entering { HOVER_ENTER_MS } else { HOVER_EXIT_MS });
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(delay).await;
            let _ = view.update(cx, |view, cx| {
                if view.hover_gen != generation {
                    return;
                }
                view.hovered = entering;
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for PipView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(entity) = self.island.upgrade() else {
            return div().size_full().into_any_element();
        };
        let view = &*self;
        let content: AnyElement = match self.kind {
            PipKind::Lyrics => super::pip::lyrics::render(view, &entity, window, cx),
            PipKind::Observe => super::pip::observe::render(view, &entity, window, cx),
        };
        div()
            .id("pip-root")
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    if event.click_count == 2 && view.kind == PipKind::Lyrics {
                        view.toggle_mini(window, cx);
                        return;
                    }
                    // macOS start_window_move is a no-op in GPUI 0.2.2, so the
                    // drag below is manual; on Linux the compositor moves.
                    // Either way the snap waits for the real button release:
                    // three-finger drag holds a ~0.5 s drag lock after the
                    // fingers lift, and snapping on frame stillness would fire
                    // while the user is still dragging through a pause.
                    view.begin_drag(window, (event.position.x.into(), event.position.y.into()), cx);
                    window.start_window_move();
                    if !cfg!(target_os = "macos") {
                        view.dragging = false;
                        view.snap_after_compositor_move(window, cx);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(
                |view, event: &MouseMoveEvent, window, _cx| {
                    if event.pressed_button != Some(MouseButton::Left) {
                        return;
                    }
                    view.drag_move(window, (event.position.x.into(), event.position.y.into()));
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _event: &MouseUpEvent, window, cx| {
                    view.end_drag(window, cx);
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _event: &MouseUpEvent, window, cx| {
                    view.end_drag(window, cx);
                    cx.notify();
                }),
            )
            .on_hover(cx.listener(|view, hovered: &bool, _window, cx| {
                view.arm_hover(*hovered, cx);
            }))
            .child(super::pip::chrome::surface(view, content))
            .into_any_element()
    }
}

fn open_with_state(
    kind: PipKind,
    island: WeakEntity<crate::island::Island>,
    corner: PipCorner,
    mini: bool,
    keep_on_top: bool,
    gallery: bool,
    cx: &mut App,
) {
    if let Some((handle, _)) = OPEN.with(|open| open.borrow().get(&kind).copied()) {
        handle
            .update(cx, |_, window, _| crate::platform::order_pip_front(window))
            .ok();
        return;
    }
    let Some(entity) = island.upgrade() else {
        return;
    };
    let force_hover = gallery && std::env::var("NOOK_GALLERY_PIP_HOVER").as_deref() == Ok("1");
    let provisional = PipView {
        kind,
        island: island.clone(),
        hovered: force_hover,
        mini,
        gallery,
        corner,
        keep_on_top,
        content_height: 240.0,
        window_number: -1,
        requested_frame: None,
        hover_gen: 0,
        island_subscription: None,
        dragging: false,
        drag_moved: false,
        drag_grab: None,
        drag_last_pointer: None,
        drag_gen: 0,
        drag_samples: Vec::new(),
        settle_gen: 0,
    };
    let (content_height, width, reduce_motion) = {
        let island_ref = entity.read(cx);
        let content_height = match kind {
            PipKind::Lyrics if mini => super::pip::lyrics::MINI_H,
            PipKind::Lyrics => super::pip::lyrics::content_height(&provisional, island_ref),
            PipKind::Observe => super::pip::observe::content_height(&provisional, island_ref),
        };
        let width = match kind {
            PipKind::Lyrics if mini => super::pip::lyrics::MINI_W,
            PipKind::Lyrics => super::pip::lyrics::LYRICS_W,
            PipKind::Observe => super::pip::observe::OBSERVE_W,
        };
        (content_height, width, island_ref.reduce_motion)
    };
    let bounds = Bounds::centered(None, size(px(width), px(content_height)), cx);
    let island_clone = island.clone();
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            window_min_size: Some(size(px(width), px(content_height))),
            ..Default::default()
        },
        move |_, cx| {
            cx.new(|cx| {
                let subscription =
                    cx.observe(&entity, |_, _, cx: &mut Context<PipView>| cx.notify());
                PipView {
                    kind,
                    island: island_clone,
                    hovered: force_hover,
                    mini,
                    gallery,
                    corner,
                    keep_on_top,
                    content_height,
                    window_number: -1,
                    requested_frame: None,
                    hover_gen: 0,
                    island_subscription: Some(subscription),
                    dragging: false,
                    drag_moved: false,
                    drag_grab: None,
                    drag_last_pointer: None,
                    drag_gen: 0,
                    drag_samples: Vec::new(),
                    settle_gen: 0,
                }
            })
        },
    );
    let Ok(handle) = opened else {
        log::error!("pip: failed to open {kind:?} window");
        return;
    };
    // Read-only number lookup inside the lease; every mutating AppKit call
    // (style + placement) runs after, via the main-queue flush in
    // platform.rs, so AppKit's resize/move callback never fires while this
    // window is leased. `window_bounds` above already carries the final
    // width x content_height, so no resize is needed before first paint.
    let number = handle
        .update(cx, |view, window, _| {
            let number = crate::platform::pip_window_number(window).unwrap_or(-1);
            view.window_number = number;
            number
        })
        .unwrap_or(-1);
    crate::platform::register_pip_window_number(number);
    crate::platform::style_pip_by_number(number, keep_on_top);
    if let Some((tx, ty, w, h)) =
        place_new_window(corner, gallery, number, width as f64, content_height as f64, reduce_motion)
    {
        // Seed the requested frame so the first drag/resize reasons from
        // the target, not the pre-flush centered bounds.
        handle
            .update(cx, |view, _, _| {
                view.note_requested(tx, ty, w, h);
            })
            .ok();
    }
    OPEN.with(|open| {
        open.borrow_mut().insert(kind, (handle, number));
    });
    if !gallery {
        persist_upsert(PipWindowState {
            kind,
            corner,
            mini,
            keep_on_top,
        });
    }
}

fn place_new_window(
    corner: PipCorner,
    gallery: bool,
    number: i64,
    width: f64,
    height: f64,
    reduce_motion: bool,
) -> Option<(f64, f64, f64, f64)> {
    // Async placement by windowNumber: lands on the main queue after
    // `open_window` returns, outside any GPUI lease.
    let visible = crate::platform::pip_visible_frame_by_number(number);
    let Some(visible) = visible else {
        return None;
    };
    let area = (visible.0 as f32, visible.1 as f32, visible.2 as f32, visible.3 as f32);
    let (tx, ty) = corner_origin(corner, (width as f32, height as f32), area, PIP_MARGIN);
    // Gallery shots and reduce-motion: land on the target corner immediately
    // (no notch drop animation). The coalescing flush keeps only the latest
    // frame per window, so the open path enqueues exactly one move; AppKit
    // animates it from the centered `open_window` bounds.
    if gallery || reduce_motion {
        crate::platform::snap_pip_frame_by_number(number, tx as f64, ty as f64, width, height, false);
        return Some((tx as f64, ty as f64, width, height));
    }
    crate::platform::snap_pip_frame_by_number(number, tx as f64, ty as f64, width, height, true);
    Some((tx as f64, ty as f64, width, height))
}

pub fn nearest_corner(
    window: (f32, f32, f32, f32),
    visible: (f32, f32, f32, f32),
) -> PipCorner {
    let (wx, wy, ww, wh) = window;
    let (vx, vy, vw, vh) = visible;
    let left = wx + ww / 2.0 < vx + vw / 2.0;
    let top = wy + wh / 2.0 > vy + vh / 2.0;
    match (top, left) {
        (true, true) => PipCorner::TopLeft,
        (true, false) => PipCorner::TopRight,
        (false, true) => PipCorner::BottomLeft,
        (false, false) => PipCorner::BottomRight,
    }
}

/// Flick projection: where the window would coast to after a decelerating
/// release, like iOS PiP / Arc. Snapping to the corner nearest this point
/// (not the release point) lets a quick throw reach another corner.
pub fn project_release(pos: (f32, f32), velocity: (f32, f32)) -> (f32, f32) {
    (
        pos.0 + velocity.0 * RELEASE_PROJECTION_S,
        pos.1 + velocity.1 * RELEASE_PROJECTION_S,
    )
}

/// Release velocity from origin samples `(seconds, x, y)` in Cocoa points.
/// First-to-last slope over the sample window; near-zero spans and absurd
/// spikes both read as a gentle drop.
pub fn sampled_velocity(samples: &[(f64, f32, f32)]) -> (f32, f32) {
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
        return (0.0, 0.0);
    };
    let dt = (last.0 - first.0) as f32;
    if dt < 0.001 {
        return (0.0, 0.0);
    }
    let mut velocity = ((last.1 - first.1) / dt, (last.2 - first.2) / dt);
    let speed = velocity.0.hypot(velocity.1);
    if speed > MAX_FLICK_SPEED {
        let scale = MAX_FLICK_SPEED / speed;
        velocity = (velocity.0 * scale, velocity.1 * scale);
    }
    velocity
}

/// Screen index + corner for a point. Screens are visibleFrames in Cocoa
/// coordinates; the point lands on the nearest screen, so a throw can carry
/// the window to another display. Empty input falls back to BottomRight.
pub fn corner_for_point(
    point: (f32, f32),
    screens: &[(f32, f32, f32, f32)],
) -> (usize, PipCorner) {
    if screens.is_empty() {
        return (0, PipCorner::BottomRight);
    }
    let mut best = 0;
    let mut best_distance = f32::MAX;
    for (index, screen) in screens.iter().enumerate() {
        let (x, y, w, h) = *screen;
        let dx = (point.0 - (x + w / 2.0)).abs() - w / 2.0;
        let dy = (point.1 - (y + h / 2.0)).abs() - h / 2.0;
        let distance = dx.max(dy).max(0.0);
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    let corner = nearest_corner((point.0, point.1, 0.0, 0.0), screens[best]);
    (best, corner)
}

pub fn corner_origin(
    corner: PipCorner,
    size: (f32, f32),
    visible: (f32, f32, f32, f32),
    margin: f32,
) -> (f32, f32) {
    let (w, h) = size;
    let (vx, vy, vw, vh) = visible;
    match corner {
        PipCorner::TopLeft => (vx + margin, vy + vh - margin - h),
        PipCorner::TopRight => (vx + vw - margin - w, vy + vh - margin - h),
        PipCorner::BottomLeft => (vx + margin, vy + margin),
        PipCorner::BottomRight => (vx + vw - margin - w, vy + margin),
    }
}

fn anchored_origin(
    corner: PipCorner,
    current: (f64, f64, f64, f64),
    size: (f64, f64),
) -> (f64, f64) {
    let (x, y, old_w, old_h) = current;
    let (w, h) = size;
    let nx = match corner {
        PipCorner::TopLeft | PipCorner::BottomLeft => x,
        PipCorner::TopRight | PipCorner::BottomRight => x + old_w - w,
    };
    let ny = match corner {
        PipCorner::BottomLeft | PipCorner::BottomRight => y,
        PipCorner::TopLeft | PipCorner::TopRight => y + old_h - h,
    };
    (nx, ny)
}

fn persisted_state(kind: PipKind) -> (PipCorner, bool, bool) {
    nook_core::settings::get_app_settings()
        .pip_windows
        .iter()
        .find(|state| state.kind == kind)
        .map(|state| (state.corner, state.mini && kind == PipKind::Lyrics, state.keep_on_top))
        .unwrap_or((PipCorner::BottomRight, false, true))
}

fn persist_upsert(state: PipWindowState) {
    nook_core::settings::tweak_app_settings(|settings| {
        if let Some(existing) = settings.pip_windows.iter_mut().find(|s| s.kind == state.kind) {
            *existing = state;
        } else {
            settings.pip_windows.push(state);
        }
    });
}

fn persist_mutation(kind: PipKind, mutate: impl FnOnce(&mut PipWindowState)) {
    nook_core::settings::tweak_app_settings(|settings| {
        if let Some(existing) = settings.pip_windows.iter_mut().find(|s| s.kind == kind) {
            mutate(existing);
        } else {
            let mut state = PipWindowState {
                kind,
                corner: PipCorner::BottomRight,
                mini: false,
                keep_on_top: true,
            };
            mutate(&mut state);
            settings.pip_windows.push(state);
        }
    });
}

impl PipView {
    fn persist(&self, mutate: impl FnOnce(&mut PipWindowState)) {
        if self.gallery {
            return;
        }
        persist_mutation(self.kind, mutate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VISIBLE: (f32, f32, f32, f32) = (0.0, 0.0, 1440.0, 900.0);

    #[test]
    fn corners_resolve_to_all_four_quadrants() {
        assert_eq!(nearest_corner((100.0, 700.0, 300.0, 200.0), VISIBLE), PipCorner::TopLeft);
        assert_eq!(nearest_corner((1000.0, 700.0, 300.0, 200.0), VISIBLE), PipCorner::TopRight);
        assert_eq!(nearest_corner((100.0, 100.0, 300.0, 200.0), VISIBLE), PipCorner::BottomLeft);
        assert_eq!(nearest_corner((1000.0, 100.0, 300.0, 200.0), VISIBLE), PipCorner::BottomRight);
    }

    #[test]
    fn corners_use_window_centre_not_origin() {
        assert_eq!(nearest_corner((600.0, 100.0, 300.0, 200.0), VISIBLE), PipCorner::BottomRight);
        assert_eq!(nearest_corner((500.0, 650.0, 300.0, 200.0), VISIBLE), PipCorner::TopLeft);
    }

    #[test]
    fn corners_respect_multi_monitor_offset_origins() {
        let right: (f32, f32, f32, f32) = (2560.0, 0.0, 1920.0, 1080.0);
        assert_eq!(nearest_corner((2600.0, 800.0, 300.0, 200.0), right), PipCorner::TopLeft);
        assert_eq!(nearest_corner((4100.0, 100.0, 300.0, 200.0), right), PipCorner::BottomRight);
        let left: (f32, f32, f32, f32) = (-1920.0, -200.0, 1920.0, 1080.0);
        assert_eq!(nearest_corner((-1800.0, 600.0, 300.0, 200.0), left), PipCorner::TopLeft);
        assert_eq!(nearest_corner((-500.0, -100.0, 300.0, 200.0), left), PipCorner::BottomRight);
    }

    #[test]
    fn origins_land_all_four_corners_with_margin() {
        let size = (300.0, 200.0);
        assert_eq!(corner_origin(PipCorner::TopLeft, size, VISIBLE, 16.0), (16.0, 684.0));
        assert_eq!(corner_origin(PipCorner::TopRight, size, VISIBLE, 16.0), (1124.0, 684.0));
        assert_eq!(corner_origin(PipCorner::BottomLeft, size, VISIBLE, 16.0), (16.0, 16.0));
        assert_eq!(corner_origin(PipCorner::BottomRight, size, VISIBLE, 16.0), (1124.0, 16.0));
    }

    #[test]
    fn origins_respect_offset_origins_and_custom_margins() {
        let right: (f32, f32, f32, f32) = (2560.0, 0.0, 1920.0, 1080.0);
        assert_eq!(
            corner_origin(PipCorner::BottomRight, (300.0, 200.0), right, 16.0),
            (4164.0, 16.0)
        );
        assert_eq!(
            corner_origin(PipCorner::TopLeft, (300.0, 200.0), right, 32.0),
            (2592.0, 848.0)
        );
    }

    #[test]
    fn snapped_origins_round_trip_through_nearest_corner() {
        let size = (300.0, 240.0);
        for corner in [
            PipCorner::TopLeft,
            PipCorner::TopRight,
            PipCorner::BottomLeft,
            PipCorner::BottomRight,
        ] {
            let (x, y) = corner_origin(corner, size, VISIBLE, PIP_MARGIN);
            assert_eq!(nearest_corner((x, y, size.0, size.1), VISIBLE), corner);
        }
    }
}
