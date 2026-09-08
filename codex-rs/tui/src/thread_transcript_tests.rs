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
    let command = vec!["zsh".to_string(), "-lc".to_string(), script];
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
        aggregated_output: Some(format!(
            "{}\nWORKSPACE_SKILL_CWD_SENTINEL\nWORKSPACE_SKILL_BODY_SENTINEL\n",
            cwd.as_path().display()
        )),
        exit_code: Some(0),
        duration_ms: Some(1),
    };

    let projection = workspace_thread_items_to_transcript_cells_with_required_skill_cwds(
        None,
        &cwd,
        [item],
        RawReasoningVisibility::Hidden,
        None,
        catalog,
    );
    let cells = projection.cells;
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

#[test]
fn workspace_skill_persisted_command_requests_its_unloaded_cwd() {
    let cwd = test_path_buf("/tmp/workspace-skill-history-unloaded").abs();
    let skill_path = test_path_buf("/tmp/enabled-skill/SKILL.md").abs();
    let command = vec![
        "zsh".to_string(),
        "-lc".to_string(),
        format!("pwd && sed -n '1,240p' {}", skill_path.display()),
    ];
    let item = ThreadItem::CommandExecution {
        id: "persisted-workspace-skill-unloaded".to_string(),
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
        aggregated_output: Some("WORKSPACE_SKILL_BODY_SENTINEL\n".to_string()),
        exit_code: Some(0),
        duration_ms: Some(1),
    };

    let projection = workspace_thread_items_to_transcript_cells_with_required_skill_cwds(
        None,
        &cwd,
        [item],
        RawReasoningVisibility::Hidden,
        None,
        Arc::new(WorkspaceSkillCatalog::default()),
    );

    assert_eq!(projection.cells.len(), 1);
    assert_eq!(projection.required_skill_cwds, vec![cwd.to_path_buf()]);
}

#[test]
fn workspace_skill_persisted_user_shell_keeps_full_output() {
    let cwd = test_path_buf("/tmp/workspace-skill-history-user-shell").abs();
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
    let command = vec![
        "zsh".to_string(),
        "-lc".to_string(),
        format!("pwd && sed -n '1,240p' {}", skill_path.display()),
    ];
    let item = ThreadItem::CommandExecution {
        id: "persisted-workspace-skill-user-shell".to_string(),
        plugin_id: None,
        script_path: None,
        command: codex_shell_command::parse_command::shlex_join(&command),
        cwd: cwd.clone().into(),
        process_id: None,
        source: CommandExecutionSource::UserShell,
        status: CommandExecutionStatus::Completed,
        command_actions: codex_shell_command::parse_command::parse_command(&command)
            .into_iter()
            .map(|parsed| CommandAction::from_core_with_cwd(parsed, &cwd))
            .collect(),
        aggregated_output: Some("USER_SHELL_SKILL_BODY_SENTINEL\n".to_string()),
        exit_code: Some(0),
        duration_ms: Some(1),
    };

    let projection = workspace_thread_items_to_transcript_cells_with_required_skill_cwds(
        None,
        &cwd,
        [item],
        RawReasoningVisibility::Hidden,
        None,
        catalog,
    );
    let workspace = projection.cells[0]
        .workspace_transcript_hyperlink_lines(/*width*/ 80)
        .into_iter()
        .flat_map(|line| line.line.spans)
        .map(|span| span.content.into_owned())
        .collect::<String>();

    assert!(workspace.contains("USER_SHELL_SKILL_BODY_SENTINEL"));
    assert!(projection.required_skill_cwds.is_empty());
}

#[test]
fn workspace_skill_persisted_command_without_exit_code_keeps_full_output() {
    let cwd = test_path_buf("/tmp/workspace-skill-history-missing-exit").abs();
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
    let command = vec![
        "zsh".to_string(),
        "-lc".to_string(),
        format!("pwd && sed -n '1,240p' {}", skill_path.display()),
    ];
    let item = ThreadItem::CommandExecution {
        id: "persisted-workspace-skill-missing-exit".to_string(),
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
        aggregated_output: Some("MISSING_EXIT_SKILL_BODY_SENTINEL\n".to_string()),
        exit_code: None,
        duration_ms: Some(1),
    };

    let projection = workspace_thread_items_to_transcript_cells_with_required_skill_cwds(
        None,
        &cwd,
        [item],
        RawReasoningVisibility::Hidden,
        None,
        catalog,
    );
    let workspace = projection.cells[0]
        .workspace_transcript_hyperlink_lines(/*width*/ 80)
        .into_iter()
        .flat_map(|line| line.line.spans)
        .map(|span| span.content.into_owned())
        .collect::<String>();

    assert!(workspace.contains("MISSING_EXIT_SKILL_BODY_SENTINEL"));
}

#[test]
fn workspace_skill_command_with_an_extra_operand_keeps_full_output() {
    let cwd = test_path_buf("/tmp/workspace-skill-history-extra-operand").abs();
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
    let command = vec![
        "zsh".to_string(),
        "-lc".to_string(),
        format!(
            "pwd && sed -n '1,240p' {} /tmp/extra-output",
            skill_path.display()
        ),
    ];
    let item = ThreadItem::CommandExecution {
        id: "persisted-workspace-skill-extra-operand".to_string(),
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
        aggregated_output: Some("EXTRA_OPERAND_OUTPUT_SENTINEL\n".to_string()),
        exit_code: Some(0),
        duration_ms: Some(1),
    };

    let projection = workspace_thread_items_to_transcript_cells_with_required_skill_cwds(
        None,
        &cwd,
        [item],
        RawReasoningVisibility::Hidden,
        None,
        catalog,
    );
    let workspace = projection.cells[0]
        .workspace_transcript_hyperlink_lines(/*width*/ 80)
        .into_iter()
        .flat_map(|line| line.line.spans)
        .map(|span| span.content.into_owned())
        .collect::<String>();

    assert!(workspace.contains("EXTRA_OPERAND_OUTPUT_SENTINEL"));
}

#[test]
fn workspace_skill_reference_document_keeps_full_output() {
    let cwd = test_path_buf("/tmp/workspace-skill-history-reference-document").abs();
    let skill_path = test_path_buf("/tmp/enabled-skill/SKILL.md").abs();
    let reference_path = test_path_buf("/tmp/enabled-skill/references/private.md").abs();
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
                path: skill_path,
                scope: crate::test_support::skill_scope_repo(),
                enabled: true,
                plugin_id: None,
            }],
            errors: Vec::new(),
        }],
    });
    let command = vec![
        "zsh".to_string(),
        "-lc".to_string(),
        format!("pwd && sed -n '1,240p' {}", reference_path.display()),
    ];
    let item = ThreadItem::CommandExecution {
        id: "persisted-workspace-skill-reference-document".to_string(),
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
        aggregated_output: Some("REFERENCE_DOCUMENT_OUTPUT_SENTINEL\n".to_string()),
        exit_code: Some(0),
        duration_ms: Some(1),
    };

    let projection = workspace_thread_items_to_transcript_cells_with_required_skill_cwds(
        None,
        &cwd,
        [item],
        RawReasoningVisibility::Hidden,
        None,
        catalog,
    );
    let workspace = projection.cells[0]
        .workspace_transcript_hyperlink_lines(/*width*/ 80)
        .into_iter()
        .flat_map(|line| line.line.spans)
        .map(|span| span.content.into_owned())
        .collect::<String>();

    assert!(workspace.contains("REFERENCE_DOCUMENT_OUTPUT_SENTINEL"));
}
