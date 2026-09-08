use super::*;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use crate::workspace_skill_output::WorkspaceSkillCatalog;
use codex_app_server_protocol::CommandAction;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::SkillMetadata;
use codex_app_server_protocol::SkillsListEntry;
use codex_app_server_protocol::SkillsListResponse;
use codex_app_server_protocol::ThreadItem;
use std::sync::Arc;

#[test]
fn workspace_skill_persisted_command_compacts_only_workspace_output() {
    let cwd = test_path_buf("/tmp/workspace-skill-history").abs();
    let skill_path = test_path_buf("/tmp/enabled-skill/SKILL.md").abs();
    let catalog = Arc::new(WorkspaceSkillCatalog::default());
    catalog.sync_response(&SkillsListResponse {
        data: vec![SkillsListEntry {
            cwd: cwd.to_path_buf(),
            skills: vec![SkillMetadata {
                name: "demo".to_string(),
                description: "test skill".to_string(),
                short_description: None,
                interface: None,
                dependencies: None,
                path: skill_path.clone(),
                scope: crate::test_support::skill_scope_repo(),
                enabled: true,
                plugin_id: None,
            }],
            errors: Vec::new(),
        }],
    });

    let script = format!("pwd && sed -n '1,240p' {}", skill_path.display());
    let command = vec!["zsh".to_string(), "-lc".to_string(), script.clone()];
    let item = ThreadItem::CommandExecution {
        id: "persisted-workspace-skill".to_string(),
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
        aggregated_output: Some(
            "WORKSPACE_SKILL_CWD_SENTINEL\nWORKSPACE_SKILL_BODY_SENTINEL\n".to_string(),
        ),
        exit_code: Some(0),
        duration_ms: Some(1),
    };

    let cells = workspace_thread_items_to_transcript_cells(
        None,
        &cwd,
        [item],
        RawReasoningVisibility::Hidden,
        None,
        catalog,
    );
    assert_eq!(cells.len(), 1);
    let transcript = cells[0]
        .transcript_lines(/*width*/ 80)
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<String>();
    let workspace = cells[0]
        .workspace_transcript_hyperlink_lines(/*width*/ 80)
        .into_iter()
        .flat_map(|line| line.line.spans)
        .map(|span| span.content.into_owned())
        .collect::<String>();

    assert!(transcript.contains("WORKSPACE_SKILL_CWD_SENTINEL"));
    assert!(transcript.contains("WORKSPACE_SKILL_BODY_SENTINEL"));
    assert!(workspace.contains("Read SKILL.md (demo skill)"));
    assert!(!workspace.contains("WORKSPACE_SKILL_CWD_SENTINEL"));
    assert!(!workspace.contains("WORKSPACE_SKILL_BODY_SENTINEL"));
}
