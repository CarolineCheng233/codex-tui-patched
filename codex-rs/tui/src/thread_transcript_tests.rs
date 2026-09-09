use super::*;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::CommandAction;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::WebSearchAction;
use codex_app_server_protocol::WebSearchItem;
use pretty_assertions::assert_eq;

fn joined_workspace_lines(cell: &dyn HistoryCell) -> String {
    cell.workspace_transcript_hyperlink_lines(/*width*/ 80)
        .into_iter()
        .map(|line| {
            line.line
                .spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn workspace_parity_persisted_read_default_display() {
    let cwd = test_path_buf("/tmp/workspace-persisted-read").abs();
    let command = vec![
        "sed".to_string(),
        "-n".to_string(),
        "1,240p".to_string(),
        "/tmp/demo/SKILL.md".to_string(),
    ];
    let item = ThreadItem::CommandExecution {
        id: "persisted-read".to_string(),
        plugin_id: None,
        script_path: None,
        command: codex_shell_command::parse_command::shlex_join(&command),
        cwd: cwd.clone().into(),
        process_id: None,
        source: CommandExecutionSource::Agent,
        status: CommandExecutionStatus::Completed,
        command_actions: codex_shell_command::parse_command::parse_command(&command)
            .into_iter()
            .map(|parsed| CommandAction::from_core_with_cwd(parsed, &cwd))
            .collect(),
        aggregated_output: Some("PERSISTED_READ_BODY_SENTINEL\n".to_string()),
        exit_code: Some(0),
        duration_ms: Some(1),
    };

    let cells =
        thread_items_to_transcript_cells(None, &cwd, [item], RawReasoningVisibility::Hidden, None);

    assert_eq!(cells.len(), 1);
    let workspace = joined_workspace_lines(cells[0].as_ref());
    assert_eq!(workspace, "• Explored\n  └ Read SKILL.md");
    insta::assert_snapshot!(workspace, @r"
    • Explored
      └ Read SKILL.md
    ");
    let detail = cells[0]
        .transcript_lines(/*width*/ 80)
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<String>();
    assert!(detail.contains("PERSISTED_READ_BODY_SENTINEL"));
}

#[test]
fn workspace_parity_persisted_detail_preserves_optional_completion_fields() {
    let cwd = test_path_buf("/tmp/workspace-persisted-detail").abs();
    let command = vec!["printf".to_string(), "done".to_string()];
    let item = ThreadItem::CommandExecution {
        id: "persisted-detail".to_string(),
        plugin_id: None,
        script_path: None,
        command: codex_shell_command::parse_command::shlex_join(&command),
        cwd: cwd.clone().into(),
        process_id: None,
        source: CommandExecutionSource::Agent,
        status: CommandExecutionStatus::Completed,
        command_actions: Vec::new(),
        aggregated_output: Some("PERSISTED_DETAIL_BODY_SENTINEL\n".to_string()),
        exit_code: None,
        duration_ms: Some(1250),
    };

    let cells =
        thread_items_to_transcript_cells(None, &cwd, [item], RawReasoningVisibility::Hidden, None);
    let detail = cells[0]
        .transcript_lines(/*width*/ 80)
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<String>();

    assert!(detail.contains("status: Completed · 1.25s"));
    assert!(!detail.contains("exit 0"));
    assert!(detail.contains("PERSISTED_DETAIL_BODY_SENTINEL"));
}

#[test]
fn workspace_parity_persisted_web_search_uses_normal_display() {
    let cwd = test_path_buf("/tmp/workspace-persisted-search").abs();
    let item = ThreadItem::WebSearch(WebSearchItem {
        id: "persisted-search".to_string(),
        query: "Codex TUI".to_string(),
        action: Some(WebSearchAction::Search {
            query: Some("Codex TUI".to_string()),
            queries: None,
        }),
        results: None,
    });

    let cells =
        thread_items_to_transcript_cells(None, &cwd, [item], RawReasoningVisibility::Hidden, None);

    let workspace = joined_workspace_lines(cells[0].as_ref());
    assert_eq!(workspace, "• Searched the web for Codex TUI");
    insta::assert_snapshot!(workspace, @"• Searched the web for Codex TUI");
}
