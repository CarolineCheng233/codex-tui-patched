//! Workspace-only presentation for confirmed skill-document reads.
//!
//! This module deliberately keeps command classification separate from the public command parser
//! and from the normal transcript. A presentation can compact only after an unambiguous command
//! shape, a successful completion, and a loaded enabled-skill catalog all agree.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::SkillMetadata;
use codex_app_server_protocol::SkillsListResponse;
use codex_protocol::parse_command::ParsedCommand;
use codex_skills::ImplicitSkillAccess;
use codex_utils_path_uri::PathUri;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum WorkspaceCommandOutcome {
    #[default]
    Pending,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceReadSummary {
    pub(crate) name: String,
}

#[derive(Clone, Debug)]
struct WorkspaceSkillReadCandidate {
    document: PathUri,
    filename: String,
    cwd: PathUri,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceSkillReadPresentation {
    candidate: WorkspaceSkillReadCandidate,
    outcome: WorkspaceCommandOutcome,
    catalog: Arc<WorkspaceSkillCatalog>,
}

impl WorkspaceSkillReadPresentation {
    pub(crate) fn summary(&self) -> Option<WorkspaceReadSummary> {
        (self.outcome == WorkspaceCommandOutcome::Succeeded)
            .then(|| self.catalog.summary_for(&self.candidate))
            .flatten()
    }

    pub(crate) fn set_outcome(&mut self, outcome: WorkspaceCommandOutcome) {
        self.outcome = outcome;
    }

    pub(crate) fn begin_catalog_refresh_if_needed(&self) -> Option<std::path::PathBuf> {
        self.catalog
            .begin_refresh_if_unrequested(&self.candidate.cwd)
            .then(|| self.candidate.cwd.to_abs_path().ok())
            .flatten()
            .map(|cwd| cwd.to_path_buf())
    }
}

#[derive(Clone, Debug)]
struct WorkspaceSkillRoot {
    document: PathUri,
    root: PathUri,
    name: String,
}

#[derive(Clone, Debug, Default)]
enum WorkspaceCatalogState {
    #[default]
    Unrequested,
    Loading,
    Ready(Vec<WorkspaceSkillRoot>),
    Failed,
}

#[derive(Debug, Default)]
pub(crate) struct WorkspaceSkillCatalog {
    states: RwLock<HashMap<PathUri, WorkspaceCatalogState>>,
    generations: RwLock<HashMap<PathUri, u64>>,
}

impl WorkspaceSkillCatalog {
    pub(crate) fn sync_response(&self, response: &SkillsListResponse) {
        let Ok(mut states) = self.states.write() else {
            return;
        };

        for entry in &response.data {
            let Ok(cwd) = PathUri::from_host_native_path(&entry.cwd) else {
                continue;
            };
            let state = if entry.errors.is_empty() {
                WorkspaceCatalogState::Ready(
                    entry
                        .skills
                        .iter()
                        .filter(|skill| skill.enabled)
                        .filter_map(WorkspaceSkillRoot::from_skill)
                        .collect(),
                )
            } else {
                WorkspaceCatalogState::Failed
            };
            states.insert(cwd, state);
        }
    }

    pub(crate) fn begin_refresh_if_unrequested(&self, cwd: &PathUri) -> bool {
        let Ok(mut states) = self.states.write() else {
            return false;
        };
        match states.entry(cwd.clone()).or_default() {
            WorkspaceCatalogState::Unrequested => {
                states.insert(cwd.clone(), WorkspaceCatalogState::Loading);
                if let Ok(mut generations) = self.generations.write() {
                    let generation = generations.entry(cwd.clone()).or_default();
                    *generation = generation.saturating_add(1);
                }
                true
            }
            WorkspaceCatalogState::Loading
            | WorkspaceCatalogState::Ready(_)
            | WorkspaceCatalogState::Failed => false,
        }
    }

    pub(crate) fn mark_failed_for_cwds(&self, cwds: &[std::path::PathBuf]) {
        let Ok(mut states) = self.states.write() else {
            return;
        };
        for cwd in cwds {
            if let Ok(cwd) = PathUri::from_host_native_path(cwd) {
                states.insert(cwd, WorkspaceCatalogState::Failed);
            }
        }
    }

    pub(crate) fn invalidate_all(&self) {
        let Ok(mut states) = self.states.write() else {
            return;
        };
        for state in states.values_mut() {
            *state = WorkspaceCatalogState::Unrequested;
        }
    }

    fn summary_for(&self, candidate: &WorkspaceSkillReadCandidate) -> Option<WorkspaceReadSummary> {
        let states = self.states.read().ok()?;
        let WorkspaceCatalogState::Ready(roots) = states.get(&candidate.cwd)? else {
            return None;
        };
        let root = roots
            .iter()
            .filter(|root| {
                candidate.document == root.document || candidate.document.starts_with(&root.root)
            })
            .max_by_key(|root| root.root.lexical_depth().unwrap_or_default())?;
        Some(WorkspaceReadSummary {
            name: format!("{} ({} skill)", candidate.filename, root.name),
        })
    }
}

impl WorkspaceSkillRoot {
    fn from_skill(skill: &SkillMetadata) -> Option<Self> {
        let document = PathUri::from_abs_path(&skill.path);
        let root = document.parent()?;
        Some(Self {
            document,
            root,
            name: skill.name.clone(),
        })
    }
}

pub(crate) fn classify_workspace_skill_read(
    raw_command: &str,
    command: &[String],
    cwd: PathUri,
    source: CommandExecutionSource,
    parsed: &[ParsedCommand],
    catalog: Arc<WorkspaceSkillCatalog>,
) -> Option<WorkspaceSkillReadPresentation> {
    if !matches!(
        source,
        CommandExecutionSource::Agent | CommandExecutionSource::UnifiedExecStartup
    ) {
        return None;
    }

    let accesses = codex_skills::implicit_skill_accesses_for_command(raw_command, &cwd);
    let documents = accesses
        .iter()
        .filter_map(|access| match access {
            ImplicitSkillAccess::Document(path) => Some(path),
            ImplicitSkillAccess::Script(_) => None,
        })
        .collect::<Vec<_>>();
    if documents.len() != 1
        || accesses
            .iter()
            .any(|access| matches!(access, ImplicitSkillAccess::Script(_)))
    {
        return None;
    }
    let document = documents[0].clone();

    let parsed_read_matches = |commands: &[ParsedCommand]| {
        let [ParsedCommand::Read { path, .. }] = commands else {
            return false;
        };
        path.to_str()
            .and_then(|path| cwd.join(path).ok())
            .is_some_and(|path| path == document)
    };

    let direct_read = parsed_read_matches(parsed);
    let shell_read =
        codex_shell_command::bash::parse_shell_lc_two_plain_commands_joined_by_and(command)
            .is_some_and(|(left, right)| {
                left == ["pwd"]
                    && parsed_read_matches(&codex_shell_command::parse_command::parse_command(
                        &right,
                    ))
            });
    if !direct_read && !shell_read {
        return None;
    }

    Some(WorkspaceSkillReadPresentation {
        candidate: WorkspaceSkillReadCandidate {
            filename: document.basename()?,
            document,
            cwd,
        },
        outcome: WorkspaceCommandOutcome::Pending,
        catalog,
    })
}

pub(crate) fn completion_outcome(
    status: codex_app_server_protocol::CommandExecutionStatus,
    exit_code: Option<i32>,
) -> WorkspaceCommandOutcome {
    if status == codex_app_server_protocol::CommandExecutionStatus::Completed
        && exit_code == Some(0)
    {
        WorkspaceCommandOutcome::Succeeded
    } else {
        WorkspaceCommandOutcome::Failed
    }
}
