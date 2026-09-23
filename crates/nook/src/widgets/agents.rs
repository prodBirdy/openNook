//! Coding-agent Nook pane + compact island faces.

use crate::dotmatrix;
use crate::island::ui::{nook_empty, nook_pane, scroll_body, slide_label};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, AnyElement, Context, MouseButton, MouseDownEvent, Rgba, SharedString,
};
use nook_core::agents::{AgentKind, AgentSession};

/// Compact leading box (Pencil Dz4Es).
const COMPACT_BOX: f32 = 22.0;
/// Compact LED mark (Pencil Agent Logo 20×20).
const COMPACT_FACE: f32 = 20.0;
/// Expanded LED fills the 28pt face slot (Pencil y2NoE1 / qcMcx).
const ROW_FACE: f32 = 28.0;
const ROW_H: f32 = 44.0;
const ROW_FACE_BOX: f32 = 28.0;
/// Row hairline #FFFFFF0D.
const ROW_RULE: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0x0D as f32 / 255.0,
};
const SLIDE_DOT: f32 = 5.0;
const SLIDE_DOT_GAP: f32 = 4.0;
/// Working Claude LED opacity in Pencil F1DXkw.
const FACE_WORKING_OPACITY: f32 = 0.72;
/// Waiting Codex LED opacity in Pencil F1DXkw.
const FACE_IDLE_OPACITY: f32 = 0.55;

/// Compact rotation order: sessions waiting on the user first, then working
/// ones, each group in scan order — deterministic across ticks.
fn rotation_order(agents: &[AgentSession]) -> impl Iterator<Item = &AgentSession> {
    let waiting = agents.iter().filter(|a| !a.status.is_working());
    waiting.chain(agents.iter().filter(|a| a.status.is_working()))
}

/// The agent the compact face shows at `rotation` (wraps over the session list).
pub(crate) fn face_agent(agents: &[AgentSession], rotation: usize) -> Option<&AgentSession> {
    if agents.is_empty() {
        return None;
    }
    rotation_order(agents).nth(rotation % agents.len())
}

pub(crate) fn compact_left(
    agents: &[AgentSession],
    rotation: usize,
    pixel_t: f32,
    on: Rgba,
    lite: bool,
) -> AnyElement {
    let agent = face_agent(agents, rotation);
    let working = agent.is_some_and(|a| a.status.is_working());
    let kind = agent.map(|a| a.kind).unwrap_or(AgentKind::Grok);
    let seed = agent.map(|a| a.pid).unwrap_or(0);
    div()
        .size(px(COMPACT_BOX))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(dotmatrix::brand_element(
            kind,
            seed,
            pixel_t,
            working,
            COMPACT_FACE,
            on,
            lite,
        ))
        .into_any_element()
}

/// Compact trailing: session title + slide dots (Pencil iZPif / jRPai).
pub(crate) fn compact_right(agents: &[AgentSession], rotation: usize) -> AnyElement {
    let Some(agent) = face_agent(agents, rotation) else {
        return div().into_any_element();
    };
    let count = agents.len();
    let index = rotation % count.max(1);
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .flex_shrink_0()
        .child(
            slide_label(agent.title().to_string(), theme::BODY, true)
                .text_color(theme::LABEL),
        )
        .when(count > 1, |d| d.child(slide_dots(count, index)))
        .into_any_element()
}

fn slide_dots(count: usize, active: usize) -> impl IntoElement {
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(SLIDE_DOT_GAP))
        .h(px(22.))
        .flex_shrink_0();
    for i in 0..count.min(6) {
        row = row.child(
            div()
                .size(px(SLIDE_DOT))
                .rounded(px(3.))
                .flex_shrink_0()
                .bg(if i == active {
                    theme::LABEL
                } else {
                    theme::TERTIARY_LABEL
                }),
        );
    }
    row
}

pub(crate) fn agents_card(
    agents: &[AgentSession],
    now: f32,
    on: Rgba,
    lite: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let body = if agents.is_empty() {
        nook_empty("bot", "No agents").into_any_element()
    } else {
        let mut col = div()
            .flex()
            .flex_col()
            .w_full()
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden();
        for agent in agents {
            col = col.child(agent_row(agent, now, on, lite, cx));
        }
        scroll_body("agents-scroll", col).into_any_element()
    };
    // Pencil F1DXkw: padding 0 16, gap 0.
    nook_pane("nook-agents").w_full().px(px(16.)).child(body)
}

fn agent_row(
    agent: &AgentSession,
    now: f32,
    on: Rgba,
    lite: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let pid = agent.pid;
    let cwd = agent.cwd.clone();
    let working = agent.status.is_working();
    div()
        .id(SharedString::from(format!("agent-{pid}")))
        .w_full()
        .h(px(ROW_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .py(px(8.))
        .overflow_hidden()
        .border_b_1()
        .border_color(ROW_RULE)
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(theme::FILL_TERTIARY))
        .active(|s| s.bg(theme::FILL_SECONDARY))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                open_agent(pid, &cwd);
            }),
        )
        .child(
            div()
                .size(px(ROW_FACE_BOX))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .opacity(if working {
                    FACE_WORKING_OPACITY
                } else {
                    FACE_IDLE_OPACITY
                })
                .child(dotmatrix::brand_element(
                    agent.kind, agent.pid, now, working, ROW_FACE, on, lite,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .flex()
                .flex_col()
                .justify_center()
                .overflow_hidden()
                // Title #FFFFFF 12/15 weight 400; detail #EBEBF599 11/14.
                .child(
                    slide_label(agent.title().to_string(), theme::CALLOUT, false)
                        .w_full()
                        .text_color(theme::LABEL),
                )
                .child(
                    slide_label(agent_detail_line(agent), theme::SUBHEADLINE, false)
                        .w_full()
                        .text_color(theme::SECONDARY_LABEL),
                ),
        )
}

fn open_agent(pid: u32, cwd: &str) {
    if !nook_core::agents::focus(pid) {
        nook_core::agents::reveal(cwd);
    }
}

fn agent_detail_line(agent: &AgentSession) -> String {
    let mut parts = vec![agent.kind.label().to_string()];
    if agent
        .name
        .as_deref()
        .is_some_and(|n| n.trim() != agent.project)
    {
        parts.push(agent.project.clone());
    }
    parts.push(agent.status.label().to_string());
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nook_core::agents::{AgentKind, AgentStatus};

    fn agent(pid: u32, working: bool) -> AgentSession {
        AgentSession {
            kind: AgentKind::Grok,
            pid,
            project: "p".into(),
            cwd: "/tmp".into(),
            status: if working {
                AgentStatus::Working
            } else {
                AgentStatus::Waiting
            },
            session_id: None,
            name: Some("session".into()),
            model: None,
        }
    }

    fn running_count(agents: &[AgentSession]) -> usize {
        agents.iter().filter(|a| a.status.is_working()).count()
    }

    #[test]
    fn compact_face_counts_only_working_agents() {
        let mixed = [agent(1, true), agent(2, false), agent(3, true)];
        assert_eq!(running_count(&mixed), 2);
        let idle = [agent(1, false), agent(2, false)];
        assert_eq!(running_count(&idle), 0);
        let busy = [agent(1, true)];
        assert_eq!(running_count(&busy), 1);
    }

    #[test]
    fn face_rotation_puts_waiting_sessions_first_and_wraps() {
        let mixed = [agent(1, true), agent(2, false), agent(3, true)];
        let pids: Vec<_> = (0..4)
            .map(|r| face_agent(&mixed, r).map(|a| a.pid))
            .collect();
        assert_eq!(pids, [Some(2), Some(1), Some(3), Some(2)]);
        assert!(face_agent(&[], 3).is_none());
    }

    #[test]
    fn detail_line_is_kind_project_state() {
        let named = AgentSession {
            kind: AgentKind::Claude,
            pid: 1,
            project: "openNook".into(),
            cwd: "/tmp".into(),
            status: AgentStatus::Working,
            session_id: None,
            name: Some("island theme pass".into()),
            model: Some("opus".into()),
        };
        assert_eq!(
            agent_detail_line(&named),
            "Claude · openNook · Working"
        );
        let project_only = AgentSession {
            kind: AgentKind::Codex,
            pid: 2,
            project: "hgi-erp".into(),
            cwd: "/tmp".into(),
            status: AgentStatus::Waiting,
            session_id: None,
            name: None,
            model: None,
        };
        assert_eq!(agent_detail_line(&project_only), "Codex · Waiting");
    }
}
