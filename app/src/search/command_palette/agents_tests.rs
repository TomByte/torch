use super::*;
use crate::ai::agent::conversation::ConversationStatus;

#[test]
fn listed_agents_show_their_latest_state_without_moving() {
    let working = AgentPane::for_test(Some(ConversationStatus::InProgress), None, None);
    let idle = AgentPane::for_test(None, None, None);
    let mut source = DataSource::default();
    source.set_panes(vec![idle.clone(), working.clone()]);
    let listed = source.panes.lock().clone();
    let rows = listed
        .iter()
        .map(|pane| Item {
            panes: source.panes.clone(),
            pane: pane.clone(),
            score: 0.,
        })
        .collect::<Vec<_>>();

    let mut needs_you = AgentPane::for_test(
        Some(ConversationStatus::Blocked {
            blocked_action: String::new(),
        }),
        None,
        None,
    );
    needs_you.locator = idle.locator;
    let newcomer = AgentPane::for_test(Some(ConversationStatus::InProgress), None, None);
    source.refresh_panes(&[newcomer, needs_you, working.clone()]);

    assert_eq!(
        rows.iter()
            .map(|row| row.current_pane().state().label())
            .collect::<Vec<_>>(),
        ["working", "needs you"],
    );
    assert_eq!(
        source
            .panes
            .lock()
            .iter()
            .map(|pane| pane.locator)
            .collect::<Vec<_>>(),
        [working.locator, idle.locator],
        "rows keep their place and agents that started meanwhile wait for the next open",
    );
}
