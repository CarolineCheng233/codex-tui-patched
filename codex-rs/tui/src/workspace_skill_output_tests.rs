use super::*;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::SkillMetadata;
use codex_app_server_protocol::SkillsListEntry;
use codex_app_server_protocol::SkillsListResponse;

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
        }),
        None
    );
}
