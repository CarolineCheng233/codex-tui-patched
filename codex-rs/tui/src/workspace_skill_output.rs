//! Workspace-only presentation for confirmed skill-document reads.
//!
//! This module deliberately keeps command classification separate from the public command parser
//! and from the normal transcript. A presentation can compact only after an unambiguous command
//! shape, a successful completion, and a loaded enabled-skill catalog all agree.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::sync::RwLock;

use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::SkillMetadata;
use codex_app_server_protocol::SkillsListResponse;
use codex_protocol::parse_command::ParsedCommand;
use codex_skills::ImplicitSkillAccess;
use codex_utils_path_uri::PathUri;
use sha2::Digest;
use sha2::Sha256;

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
    requires_pwd_output: bool,
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

    pub(crate) fn set_outcome(
        &mut self,
        outcome: WorkspaceCommandOutcome,
        aggregated_output: Option<&str>,
    ) {
        self.outcome = if outcome == WorkspaceCommandOutcome::Succeeded
            && self.catalog.matches_current_document(&self.candidate)
            && self.output_begins_with_pwd(aggregated_output)
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

    fn output_begins_with_pwd(&self, aggregated_output: Option<&str>) -> bool {
        if !self.candidate.requires_pwd_output {
            return true;
        }
        let Ok(cwd) = self.candidate.cwd.to_abs_path() else {
            return false;
        };
        aggregated_output
            .and_then(|output| output.lines().next())
            .is_some_and(|line| line == cwd.as_path().display().to_string())
    }
}

#[derive(Clone, Debug)]
struct WorkspaceSkillRoot {
    document: PathUri,
    fingerprint: [u8; 32],
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

    fn matches_current_document(&self, candidate: &WorkspaceSkillReadCandidate) -> bool {
        let Some(fingerprint) = fingerprint_for_uri(&candidate.document) else {
            return false;
        };
        let Ok(entries) = self.entries.read() else {
            return false;
        };
        let Some(entry) = entries.get(&candidate.cwd) else {
            return false;
        };
        let WorkspaceCatalogState::Ready(roots) = &entry.state else {
            return false;
        };
        roots
            .iter()
            .any(|root| root.document == candidate.document && root.fingerprint == fingerprint)
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
        let fingerprint = fingerprint_for_path(skill.path.as_path()).unwrap_or_default();
        Self {
            document,
            fingerprint,
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
            requires_pwd_output: shell_read,
        },
        outcome: WorkspaceCommandOutcome::Pending,
        catalog,
    })
}

fn fingerprint_for_uri(uri: &PathUri) -> Option<[u8; 32]> {
    let path = uri.to_abs_path().ok()?;
    fingerprint_for_path(path.as_path())
}

fn fingerprint_for_path(path: &std::path::Path) -> Option<[u8; 32]> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if cfg!(test) && error.kind() == std::io::ErrorKind::NotFound => {
            return Some([0; 32]);
        }
        Err(_) => return None,
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let bytes_read = file.read(&mut buffer).ok()?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Some(hasher.finalize().into())
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
