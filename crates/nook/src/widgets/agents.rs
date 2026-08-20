//! Coding-agent card + compact island faces.

use crate::dotmatrix;
use crate::island::ui::{card_row, label, widget_shell};
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

pub(crate) fn compact_right(agents: &[AgentSession]) -> AnyElement {
    let text = if agents.len() == 1 {
        agents[0].title().to_string()
    } else {
        agents.len().to_string()
    };
    label(text, theme::BODY, true).into_any_element()
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
            let cwd = agent.cwd.clone();
            let pid = agent.pid;
            let working = agent.status.is_working();
            body = body.child(
                card_row(SharedString::from(format!("agent-{}", agent.pid)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            // Focus the terminal the agent is running in; only
                            // fall back to its folder if nothing can be focused
                            // (agent already exited, or no app-owning ancestor).
                            if !nook_core::agents::focus(pid) {
                                nook_core::agents::reveal(&cwd);
                            }
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
                            .child(label(agent.title().to_string(), theme::CALLOUT, true).w_full())
                            .child(
                                label(agent_detail_line(agent, working), theme::SUBHEADLINE, false)
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
    widget_shell("bot", "Agents", body)
}

fn agent_detail_line(agent: &AgentSession, working: bool) -> String {
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
    parts.push(if working {
        format!("{} — working", agent.status.label())
    } else {
        agent.status.label().to_string()
    });
    parts.join(" · ")
}
