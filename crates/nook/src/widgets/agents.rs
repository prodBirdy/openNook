//! Coding-agent card + compact island faces.

use crate::dotmatrix;
use crate::icons::lucide_color;
use crate::island::ui::{card_row, label, slide_label, widget_shell_actions};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, AnyElement, Context, MouseButton, MouseDownEvent, SharedString};
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
    let mut body = div().flex().flex_col().gap_1();
    if agents.is_empty() {
        body = body
            .child(label("No coding agents running", theme::CALLOUT, true))
            .child(label(
                "Start an agent in a project to see it here.",
                theme::SUBHEADLINE,
                false,
            ));
    } else {
        for agent in agents.iter().take(4) {
            let pid = agent.pid;
            let working = agent.status.is_working();
            body = body.child(
                card_row(SharedString::from(format!("agent-{}", agent.pid)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let _ = nook_core::agents::focus(pid);
                        }),
                    )
                    .child(dotmatrix::element(
                        dotmatrix::pick(agent.pid),
                        now,
                        working,
                        dotmatrix::WIDGET_SIZE,
                    ))
                    .child(
                        div()
                            .flex_1()
                            // Without this the column keeps its content width and
                            // the title overflows the card instead of truncating.
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                slide_label(agent.title().to_string(), theme::CALLOUT, true)
                                    .w_full(),
                            )
                            .child(
                                slide_label(agent_detail_line(agent), theme::SUBHEADLINE, false)
                                    .w_full(),
                            ),
                    ),
            );
        }
        if agents.len() > 4 {
            body = body.child(label(
                format!("{} more", agents.len() - 4),
                theme::SUBHEADLINE,
                false,
            ));
        }
    }
    widget_shell_actions("agents-scroll", "Agents", div(), body)
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
