use super::*;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::SkillMetadata;
use codex_app_server_protocol::SkillsListEntry;
use codex_app_server_protocol::SkillsListResponse;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::os::unix::fs::symlink;
use tempfile::tempdir;

#[test]
fn catalog_invalidation_rejects_a_response_from_its_previous_ticket() {
    let cwd_path = test_path_buf("/tmp/workspace-skill-catalog").abs();
    let cwd = PathUri::from_abs_path(&cwd_path);
    let skill_path = test_path_buf("/tmp/catalog-skill/SKILL.md").abs();
    let catalog = WorkspaceSkillCatalog::default();

    assert!(catalog.begin_refresh_if_unrequested(&cwd));
    let stale_ticket = catalog.current_ticket(&cwd).expect("refresh ticket");
    catalog.invalidate_all();

    catalog.sync_response_if_current(
        &SkillsListResponse {
            data: vec![SkillsListEntry {
                cwd: cwd_path.to_path_buf(),
                skills: vec![SkillMetadata {
                    name: "stale".to_string(),
                    description: "stale catalog response".to_string(),
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
        },
        &[stale_ticket],
    );

    assert_eq!(
        catalog.summary_for(&WorkspaceSkillReadCandidate {
            document: PathUri::from_abs_path(&skill_path),
            filename: "SKILL.md".to_string(),
            cwd,
            requires_pwd_output: false,
        }),
        None
    );
}

#[test]
fn completed_read_of_a_skill_path_replaced_by_a_symlink_stays_full() {
    let temp_dir = tempdir().expect("temp dir");
    let cwd_path = AbsolutePathBuf::from_absolute_path(temp_dir.path()).expect("absolute cwd");
    let skill_path = temp_dir.path().join("SKILL.md");
    std::fs::write(&skill_path, "trusted skill body").expect("skill document");
    let skill_path = AbsolutePathBuf::from_absolute_path(skill_path).expect("absolute skill path");
    let catalog = Arc::new(WorkspaceSkillCatalog::default());
    catalog.sync_response(&SkillsListResponse {
        data: vec![SkillsListEntry {
            cwd: cwd_path.to_path_buf(),
            skills: vec![skill_metadata(skill_path.clone())],
            errors: Vec::new(),
        }],
    });

    let target = temp_dir.path().join("outside.txt");
    std::fs::write(&target, "outside content").expect("outside document");
    std::fs::remove_file(skill_path.as_path()).expect("remove skill document");
    symlink(&target, skill_path.as_path()).expect("replace skill with symlink");

    let mut presentation = WorkspaceSkillReadPresentation {
        candidate: WorkspaceSkillReadCandidate {
            document: PathUri::from_abs_path(&skill_path),
            filename: "SKILL.md".to_string(),
            cwd: PathUri::from_abs_path(&cwd_path),
            requires_pwd_output: false,
        },
        outcome: WorkspaceCommandOutcome::Pending,
        catalog,
    };
    presentation.set_outcome(WorkspaceCommandOutcome::Succeeded, None);

    assert_eq!(presentation.summary(), None);
}

#[test]
fn shell_candidate_with_output_before_pwd_stays_full() {
    let cwd_path = test_path_buf("/tmp/workspace-skill-shell-output").abs();
    let skill_path = test_path_buf("/tmp/enabled-skill/SKILL.md").abs();
    let catalog = Arc::new(WorkspaceSkillCatalog::default());
    catalog.sync_response(&SkillsListResponse {
        data: vec![SkillsListEntry {
            cwd: cwd_path.to_path_buf(),
            skills: vec![skill_metadata(skill_path.clone())],
            errors: Vec::new(),
        }],
    });
    let mut presentation = WorkspaceSkillReadPresentation {
        candidate: WorkspaceSkillReadCandidate {
            document: PathUri::from_abs_path(&skill_path),
            filename: "SKILL.md".to_string(),
            cwd: PathUri::from_abs_path(&cwd_path),
            requires_pwd_output: true,
        },
        outcome: WorkspaceCommandOutcome::Pending,
        catalog,
    };

    presentation.set_outcome(
        WorkspaceCommandOutcome::Succeeded,
        Some("zsh startup output\n/tmp/workspace-skill-shell-output\nskill body\n"),
    );

    assert_eq!(presentation.summary(), None);
}

fn skill_metadata(path: AbsolutePathBuf) -> SkillMetadata {
    SkillMetadata {
        name: "demo".to_string(),
        description: "test skill".to_string(),
        short_description: None,
        interface: None,
        dependencies: None,
        path,
        scope: crate::test_support::skill_scope_repo(),
        enabled: true,
        plugin_id: None,
    }
}
