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
        self.outcome = if outcome == WorkspaceCommandOutcome::Succeeded
            && self.candidate_document_is_not_a_symlink()
        {
            WorkspaceCommandOutcome::Succeeded
        } else {
            WorkspaceCommandOutcome::Failed
        };
    }

    pub(crate) fn begin_catalog_refresh_if_needed(&self) -> Option<std::path::PathBuf> {
        let cwd = self.candidate.cwd.to_abs_path().ok()?.to_path_buf();
        self.catalog
            .begin_refresh_if_unrequested(&self.candidate.cwd)
            .then_some(cwd)
    }

    fn candidate_document_is_not_a_symlink(&self) -> bool {
        let Ok(path) = self.candidate.document.to_abs_path() else {
            return false;
        };
        match std::fs::symlink_metadata(path.as_path()) {
            Ok(metadata) => metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
            Err(error) if cfg!(test) && error.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => false,
        }
    }
}

#[derive(Clone, Debug)]
struct WorkspaceSkillRoot {
    document: PathUri,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceSkillRefreshTicket {
    cwd: PathUri,
    generation: u64,
}

impl WorkspaceSkillRefreshTicket {
    pub(crate) fn is_for_cwd(&self, cwd: &PathUri) -> bool {
        self.cwd == *cwd
    }
}

#[derive(Debug, Default)]
struct WorkspaceSkillCatalogEntry {
    generation: u64,
    state: WorkspaceCatalogState,
}

#[derive(Debug, Default)]
pub(crate) struct WorkspaceSkillCatalog {
    entries: RwLock<HashMap<PathUri, WorkspaceSkillCatalogEntry>>,
}

impl WorkspaceSkillCatalog {
    pub(crate) fn sync_response_if_current(
        &self,
        response: &SkillsListResponse,
        expected: &[WorkspaceSkillRefreshTicket],
    ) -> bool {
        if expected.is_empty() {
            return false;
        }
        let Ok(mut entries) = self.entries.write() else {
            return false;
        };
        if expected
            .iter()
            .any(|ticket| !Self::is_current(&entries, ticket))
        {
            return false;
        }
        for ticket in expected {
            let state = response
                .data
                .iter()
                .filter(|entry| {
                    PathUri::from_host_native_path(&entry.cwd).is_ok_and(|cwd| cwd == ticket.cwd)
                })
                .map(Self::state_from_response_entry)
                .collect::<Vec<_>>();
            let entry = entries
                .get_mut(&ticket.cwd)
                .expect("current workspace skill ticket must have a catalog entry");
            entry.state = match state.as_slice() {
                [state] => state.clone(),
                _ => WorkspaceCatalogState::Failed,
            };
        }
        true
    }

    pub(crate) fn current_ticket(&self, cwd: &PathUri) -> Option<WorkspaceSkillRefreshTicket> {
        let entries = self.entries.read().ok()?;
        let entry = entries.get(cwd)?;
        Some(WorkspaceSkillRefreshTicket {
            cwd: cwd.clone(),
            generation: entry.generation,
        })
    }

    pub(crate) fn sync_response(&self, response: &SkillsListResponse) {
        let Ok(mut entries) = self.entries.write() else {
            return;
        };

        for entry in &response.data {
            let Ok(cwd) = PathUri::from_host_native_path(&entry.cwd) else {
                continue;
            };
            entries.entry(cwd).or_default().state = Self::state_from_response_entry(entry);
        }
    }

    pub(crate) fn begin_refresh_if_unrequested(&self, cwd: &PathUri) -> bool {
        let Ok(mut entries) = self.entries.write() else {
            return false;
        };
        let entry = entries.entry(cwd.clone()).or_default();
        if !matches!(entry.state, WorkspaceCatalogState::Unrequested) {
            return false;
        }
        entry.state = WorkspaceCatalogState::Loading;
        entry.generation = entry.generation.saturating_add(1);
        true
    }

    pub(crate) fn mark_failed_for_cwds(&self, cwds: &[std::path::PathBuf]) {
        let Ok(mut entries) = self.entries.write() else {
            return;
        };
        for cwd in cwds {
            if let Ok(cwd) = PathUri::from_host_native_path(cwd) {
                entries.entry(cwd).or_default().state = WorkspaceCatalogState::Failed;
            }
        }
    }

    pub(crate) fn mark_failed_if_current(&self, tickets: &[WorkspaceSkillRefreshTicket]) -> bool {
        if tickets.is_empty() {
            return false;
        }
        let Ok(mut entries) = self.entries.write() else {
            return false;
        };
        if tickets
            .iter()
            .any(|ticket| !Self::is_current(&entries, ticket))
        {
            return false;
        }
        for ticket in tickets {
            entries
                .get_mut(&ticket.cwd)
                .expect("current workspace skill ticket must have a catalog entry")
                .state = WorkspaceCatalogState::Failed;
        }
        true
    }

    pub(crate) fn invalidate_all(&self) -> Vec<std::path::PathBuf> {
        let Ok(mut entries) = self.entries.write() else {
            return Vec::new();
        };
        let mut cwds = Vec::with_capacity(entries.len());
        for (cwd, entry) in entries.iter_mut() {
            entry.generation = entry.generation.saturating_add(1);
            entry.state = WorkspaceCatalogState::Unrequested;
            if let Ok(cwd) = cwd.to_abs_path() {
                cwds.push(cwd.to_path_buf());
            }
        }
        cwds
    }

    fn summary_for(&self, candidate: &WorkspaceSkillReadCandidate) -> Option<WorkspaceReadSummary> {
        let entries = self.entries.read().ok()?;
        let WorkspaceCatalogState::Ready(roots) = &entries.get(&candidate.cwd)?.state else {
            return None;
        };
        let root = roots
            .iter()
            .find(|root| candidate.document == root.document)?;
        Some(WorkspaceReadSummary {
            name: format!("{} ({} skill)", candidate.filename, root.name),
        })
    }

    fn is_current(
        entries: &HashMap<PathUri, WorkspaceSkillCatalogEntry>,
        ticket: &WorkspaceSkillRefreshTicket,
    ) -> bool {
        entries
            .get(&ticket.cwd)
            .is_some_and(|entry| entry.generation == ticket.generation)
    }

    fn state_from_response_entry(
        entry: &codex_app_server_protocol::SkillsListEntry,
    ) -> WorkspaceCatalogState {
        if !entry.errors.is_empty() {
            return WorkspaceCatalogState::Failed;
        }
        let roots = entry
            .skills
            .iter()
            .filter(|skill| skill.enabled)
            .map(WorkspaceSkillRoot::from_skill)
            .collect();
        WorkspaceCatalogState::Ready(roots)
    }
}

impl WorkspaceSkillRoot {
    fn from_skill(skill: &SkillMetadata) -> Self {
        let document = PathUri::from_abs_path(&skill.path);
        Self {
            document,
            name: skill.name.clone(),
        }
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

    let direct_read =
        parsed_read_matches(parsed) && is_compactable_skill_read_argv(command, &cwd, &document);
    let shell_read =
        codex_shell_command::bash::parse_shell_lc_two_plain_commands_joined_by_and(command)
            .is_some_and(|(left, right)| {
                left == ["pwd"]
                    && parsed_read_matches(&codex_shell_command::parse_command::parse_command(
                        &right,
                    ))
                    && is_compactable_skill_read_argv(&right, &cwd, &document)
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

fn is_compactable_skill_read_argv(command: &[String], cwd: &PathUri, document: &PathUri) -> bool {
    let path = match command {
        [program, path] if program == "cat" => path,
        [program, flag, range, path] if program == "sed" && flag == "-n" && range == "1,240p" => {
            path
        }
        _ => return false,
    };
    cwd.join(path).is_ok_and(|path| path == *document)
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

#[cfg(test)]
#[path = "workspace_skill_output_tests.rs"]
mod tests;
