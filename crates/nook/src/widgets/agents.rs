//! Coding-agent Nook pane + compact island faces.

use crate::dotmatrix;
use crate::icons::lucide_color;
use crate::island::ui::{label, nook_empty, nook_pane, nook_row, scroll_body, slide_label};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, rgba, AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent,
    SharedString,
};
use nook_core::agents::AgentSession;

pub(crate) fn compact_left(agents: &[AgentSession], pixel_t: f32) -> AnyElement {
    let working = agents.iter().any(|a| a.status.is_working());
    let seed = agents
        .iter()
        .find(|a| a.status.is_working())
        .or(agents.first())
        .map(|a| a.pid)
        .unwrap_or(0);
    dotmatrix::element(
        dotmatrix::pick(seed),
        pixel_t,
        working,
        dotmatrix::COMPACT_SIZE,
    )
    .into_any_element()
}

/// Running count, or a pause glyph when every session is waiting.
pub(crate) fn compact_right(agents: &[AgentSession]) -> AnyElement {
    let running = running_count(agents);
    if running == 0 {
        lucide_color("pause-fill", 14.0, theme::TEXT_MUTED).into_any_element()
    } else {
        label(running.to_string(), theme::BODY, true).into_any_element()
    }
}

fn running_count(agents: &[AgentSession]) -> usize {
    agents.iter().filter(|a| a.status.is_working()).count()
}

pub(crate) fn agents_card(
    agents: &[AgentSession],
    now: f32,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let body = if agents.is_empty() {
        nook_empty("bot", "No agents").into_any_element()
    } else {
        let mut col = div().flex().flex_col().w_full();
        for agent in agents {
            col = col.child(agent_row(agent, now, cx));
        }
        scroll_body("agents-scroll", col).into_any_element()
    };
    nook_pane("nook-agents").w_full().child(body)
}

fn agent_row(agent: &AgentSession, now: f32, cx: &mut Context<Island>) -> impl IntoElement {
    let pid = agent.pid;
    let cwd = agent.cwd.clone();
    let working = agent.status.is_working();
    nook_row(SharedString::from(format!("agent-{pid}")))
        .min_h(px(theme::HIT_MIN))
        .gap(px(8.))
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(rgba(0xFFFFFF0D)))
        .active(|s| s.opacity(0.85))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                open_agent(pid, &cwd);
            }),
        )
        .child(
            div()
                .w(px(20.))
                .flex_shrink_0()
                .flex()
                .justify_center()
                .child(dotmatrix::element(
                    dotmatrix::pick(agent.pid),
                    now,
                    working,
                    dotmatrix::WIDGET_SIZE,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .justify_center()
                .overflow_hidden()
                .child(slide_label(agent.title().to_string(), theme::CALLOUT, true).w_full())
                .child(slide_label(agent_detail_line(agent), theme::SUBHEADLINE, false).w_full()),
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
    if let Some(model) = agent
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(model.to_string());
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

    #[test]
    fn compact_face_counts_only_working_agents() {
        let mixed = [agent(1, true), agent(2, false), agent(3, true)];
        assert_eq!(running_count(&mixed), 2);
        let idle = [agent(1, false), agent(2, false)];
        assert_eq!(running_count(&idle), 0);
        let busy = [agent(1, true)];
        assert_eq!(running_count(&busy), 1);
    }
}
