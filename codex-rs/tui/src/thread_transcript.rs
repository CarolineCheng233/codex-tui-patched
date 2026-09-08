//! Render persisted thread turns into history-cell building blocks.

use std::path::PathBuf;
use std::sync::Arc;

use crate::app_server_session::AppServerSession;
use crate::app_server_session::HistoryHydrationScope;
use crate::exec_cell::workspace_read_summary_lines;
use crate::exec_command::split_command_string;
use crate::git_action_directives::parse_assistant_markdown;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::PrefixedWrappedHistoryCell;
use crate::history_cell::ReasoningSummaryCell;
use crate::history_cell::UserHistoryCell;
use crate::history_cell::split_reasoning_summary_parts;
use crate::inline_visualization::InlineVisualizationContext;
use crate::legacy_core::config::Config;
use crate::multi_agents::sub_agent_activity_summary;
use crate::terminal_hyperlinks::plain_hyperlink_lines;
use crate::workspace_skill_output::WorkspaceSkillCatalog;
use crate::workspace_skill_output::WorkspaceSkillReadPresentation;
use crate::workspace_skill_output::classify_workspace_skill_read;
use crate::workspace_skill_output::completion_outcome;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::UserInput;
use codex_protocol::ThreadId;
use codex_protocol::items::UserMessageItem;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use ratatui::style::Stylize as _;
use ratatui::text::Line;

pub(crate) type TranscriptCells = Vec<Arc<dyn HistoryCell>>;

pub(crate) struct WorkspaceTranscriptProjection {
    pub(crate) cells: TranscriptCells,
    pub(crate) required_skill_cwds: Vec<PathBuf>,
}

#[derive(Debug)]
struct WorkspaceCommandHistoryCell {
    lines: Vec<Line<'static>>,
    workspace_skill_read: Option<WorkspaceSkillReadPresentation>,
}

impl HistoryCell for WorkspaceCommandHistoryCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        self.lines.clone()
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.lines.clone()
    }

    fn workspace_transcript_hyperlink_lines(
        &self,
        width: u16,
    ) -> Vec<crate::terminal_hyperlinks::HyperlinkLine> {
        let Some(summary) = self
            .workspace_skill_read
            .as_ref()
            .and_then(WorkspaceSkillReadPresentation::summary)
        else {
            return plain_hyperlink_lines(self.lines.clone());
        };
        plain_hyperlink_lines(workspace_read_summary_lines(&[summary], width))
    }

    fn has_stable_transcript_height(&self) -> bool {
        self.workspace_skill_read.is_none()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RawReasoningVisibility {
    Hidden,
    Visible,
}

pub(crate) async fn load_session_transcript(
    app_server: &mut AppServerSession,
    thread_id: ThreadId,
    raw_reasoning_visibility: RawReasoningVisibility,
    config: Option<&Config>,
) -> std::io::Result<TranscriptCells> {
    let mut thread = app_server
        .thread_read(thread_id, /*include_turns*/ false)
        .await
        .map_err(std::io::Error::other)?;
    app_server
        .hydrate_initial_thread_history(
            &mut thread,
            /*turn_cursor*/ None,
            /*item_cursor*/ None,
            /*config*/ None,
            /*local_settings*/ None,
            HistoryHydrationScope::Complete,
        )
        .await
        .map_err(std::io::Error::other)?;
    Ok(thread_to_transcript_cells(
        thread,
        raw_reasoning_visibility,
        config,
    ))
}

pub(crate) fn thread_to_transcript_cells(
    thread: Thread,
    raw_reasoning_visibility: RawReasoningVisibility,
    config: Option<&Config>,
) -> TranscriptCells {
    let cwd = thread.cwd;
    let thread_id = ThreadId::from_string(&thread.id).ok();
    let mut cells = thread_items_to_transcript_cells(
        thread_id,
        &cwd,
        thread.turns.into_iter().flat_map(|turn| turn.items),
        raw_reasoning_visibility,
        config,
    );
    if cells.is_empty() {
        cells.push(Arc::new(PlainHistoryCell::new(vec![
            "No transcript content available".italic().dim().into(),
        ])));
    }
    cells
}

pub(crate) fn thread_items_to_transcript_cells(
    thread_id: Option<ThreadId>,
    cwd: &AbsolutePathBuf,
    items: impl IntoIterator<Item = ThreadItem>,
    raw_reasoning_visibility: RawReasoningVisibility,
    config: Option<&Config>,
) -> TranscriptCells {
    thread_items_to_transcript_cells_with_workspace_catalog(
        thread_id,
        cwd,
        items,
        raw_reasoning_visibility,
        config,
        None,
        None,
    )
}

pub(crate) fn workspace_thread_items_to_transcript_cells_with_required_skill_cwds(
    thread_id: Option<ThreadId>,
    cwd: &AbsolutePathBuf,
    items: impl IntoIterator<Item = ThreadItem>,
    raw_reasoning_visibility: RawReasoningVisibility,
    config: Option<&Config>,
    workspace_skill_catalog: Arc<WorkspaceSkillCatalog>,
) -> WorkspaceTranscriptProjection {
    let mut required_skill_cwds = Vec::new();
    let cells = thread_items_to_transcript_cells_with_workspace_catalog(
        thread_id,
        cwd,
        items,
        raw_reasoning_visibility,
        config,
        Some(workspace_skill_catalog),
        Some(&mut required_skill_cwds),
    );
    WorkspaceTranscriptProjection {
        cells,
        required_skill_cwds,
    }
}

fn thread_items_to_transcript_cells_with_workspace_catalog(
    thread_id: Option<ThreadId>,
    cwd: &AbsolutePathBuf,
    items: impl IntoIterator<Item = ThreadItem>,
    raw_reasoning_visibility: RawReasoningVisibility,
    config: Option<&Config>,
    workspace_skill_catalog: Option<Arc<WorkspaceSkillCatalog>>,
    mut required_skill_cwds: Option<&mut Vec<PathBuf>>,
) -> TranscriptCells {
    let inline_visualization_context = config.and_then(|config| {
        thread_id.and_then(|thread_id| InlineVisualizationContext::from_config(config, thread_id))
    });
    let mut cells: TranscriptCells = Vec::new();
    for item in items {
        match item {
            ThreadItem::UserMessage {
                id,
                client_id,
                content,
            } => {
                if content.iter().any(|input| {
                    matches!(
                        input,
                        UserInput::Audio { .. } | UserInput::LocalAudio { .. }
                    )
                }) {
                    tracing::warn!(
                        user_message_id = id,
                        "audio user inputs are not supported by the TUI and will be omitted"
                    );
                }
                let item = UserMessageItem {
                    id,
                    client_id,
                    content: content
                        .into_iter()
                        .map(codex_app_server_protocol::UserInput::into_core)
                        .collect(),
                };
                cells.push(Arc::new(UserHistoryCell {
                    message: item.message(),
                    text_elements: item.text_elements(),
                    local_image_paths: item.local_image_paths(),
                    remote_image_urls: item.image_urls(),
                }));
            }
            ThreadItem::AgentMessage { text, .. } => {
                let parsed = parse_assistant_markdown(&text, cwd.as_path());
                if !parsed.visible_markdown.trim().is_empty() {
                    cells.push(Arc::new(AgentMarkdownCell::new_with_inline_visualizations(
                        parsed.visible_markdown,
                        cwd.as_path(),
                        inline_visualization_context.clone(),
                    )));
                }
            }
            ThreadItem::FunctionCallOutput {
                name,
                namespace,
                output,
                ..
            } => {
                if let Some((source_thread_id, prompt)) =
                    crate::dynamic_tools::parse_delegated_tool_output(
                        &name,
                        namespace.as_deref(),
                        &output,
                    )
                {
                    cells.push(Arc::new(PrefixedWrappedHistoryCell::new(
                        format!("Sent by Codex from task {source_thread_id}\n{prompt}"),
                        "• ".dim(),
                        "  ",
                    )));
                }
            }
            ThreadItem::Plan { text, .. } => {
                if !text.trim().is_empty() {
                    cells.push(Arc::new(crate::history_cell::new_proposed_plan(
                        text,
                        cwd.as_path(),
                    )));
                }
            }
            ThreadItem::Reasoning {
                summary, content, ..
            } => {
                let (header, text) =
                    if matches!(raw_reasoning_visibility, RawReasoningVisibility::Visible)
                        && !content.is_empty()
                    {
                        ("Reasoning".to_string(), content.join("\n\n"))
                    } else {
                        split_reasoning_summary_parts(&summary)
                    };
                if !text.trim().is_empty() {
                    cells.push(Arc::new(ReasoningSummaryCell::new(
                        header,
                        text,
                        cwd.as_path(),
                        /*transcript_only*/ false,
                    )));
                }
            }
            ThreadItem::CommandExecution {
                command,
                cwd,
                source,
                status,
                command_actions,
                aggregated_output,
                exit_code,
                ..
            } if workspace_skill_catalog.is_some() => {
                let raw_command = command.clone();
                let command_argv = split_command_string(&command);
                let parsed = command_actions
                    .iter()
                    .cloned()
                    .map(codex_app_server_protocol::CommandAction::into_core)
                    .collect::<Vec<_>>();
                let workspace_skill_read = PathUri::try_from(cwd)
                    .ok()
                    .and_then(|cwd| {
                        classify_workspace_skill_read(
                            &raw_command,
                            &command_argv,
                            cwd,
                            source,
                            &parsed,
                            workspace_skill_catalog
                                .as_ref()
                                .expect("workspace catalog exists")
                                .clone(),
                        )
                    })
                    .map(|mut presentation| {
                        presentation.set_outcome(
                            completion_outcome(status.clone(), exit_code),
                            aggregated_output.as_deref(),
                        );
                        presentation
                    });
                if let Some(cwd) = workspace_skill_read
                    .as_ref()
                    .and_then(WorkspaceSkillReadPresentation::begin_catalog_refresh_if_needed)
                    && let Some(required_skill_cwds) = required_skill_cwds.as_deref_mut()
                {
                    required_skill_cwds.push(cwd);
                }
                let lines = command_execution_fallback_lines(
                    &command,
                    status,
                    aggregated_output.as_deref(),
                    exit_code,
                );
                cells.push(Arc::new(WorkspaceCommandHistoryCell {
                    lines,
                    workspace_skill_read,
                }));
            }
            other => {
                if let Some(cell) = fallback_transcript_cell(&other) {
                    cells.push(Arc::new(cell));
                }
            }
        }
    }
    cells
}

fn fallback_transcript_cell(item: &ThreadItem) -> Option<PlainHistoryCell> {
    let lines = match item {
        ThreadItem::HookPrompt { fragments, .. } => fragments
            .iter()
            .map(|fragment| {
                vec![
                    "hook prompt: ".dim(),
                    fragment.text.trim().to_string().into(),
                ]
                .into()
            })
            .collect::<Vec<_>>(),
        ThreadItem::CommandExecution {
            command,
            status,
            aggregated_output,
            exit_code,
            ..
        } => command_execution_fallback_lines(
            command,
            status.clone(),
            aggregated_output.as_deref(),
            *exit_code,
        ),
        ThreadItem::FileChange {
            changes, status, ..
        } => vec![
            format!("file changes: {status:?} · {} changes", changes.len())
                .dim()
                .into(),
        ],
        ThreadItem::McpToolCall {
            server,
            tool,
            status,
            ..
        } => vec![
            format!("mcp tool: {server}/{tool} · {status:?}")
                .dim()
                .into(),
        ],
        ThreadItem::DynamicToolCall {
            namespace,
            tool,
            status,
            ..
        } => {
            let name = namespace
                .as_ref()
                .map(|namespace| format!("{namespace}/{tool}"))
                .unwrap_or_else(|| tool.clone());
            vec![format!("tool: {name} · {status:?}").dim().into()]
        }
        ThreadItem::CollabAgentToolCall { tool, status, .. } => {
            vec![format!("agent tool: {tool:?} · {status:?}").dim().into()]
        }
        ThreadItem::SubAgentActivity {
            kind, agent_path, ..
        } => {
            vec![sub_agent_activity_summary(*kind, agent_path).dim().into()]
        }
        ThreadItem::WebSearch(item) => {
            vec![vec!["web search: ".dim(), item.query.clone().into()].into()]
        }
        ThreadItem::ImageView { path, .. } => {
            let path = path.render_for_ui();
            vec![format!("image: {path}").dim().into()]
        }
        ThreadItem::ImageGeneration(item) => {
            let saved = item
                .saved_path
                .as_ref()
                .map(|path| format!(" · {}", path.as_path().display()))
                .unwrap_or_default();
            vec![
                format!("image generation: {}{saved}", item.status)
                    .dim()
                    .into(),
            ]
        }
        ThreadItem::EnteredReviewMode { review, .. } => {
            vec![vec!["review started: ".dim(), review.clone().into()].into()]
        }
        ThreadItem::ExitedReviewMode { review, .. } => {
            vec![vec!["review finished: ".dim(), review.clone().into()].into()]
        }
        ThreadItem::ContextCompaction { .. } => {
            vec!["context compacted".dim().into()]
        }
        ThreadItem::UserMessage { .. }
        | ThreadItem::AgentMessage { .. }
        | ThreadItem::FunctionCallOutput { .. }
        | ThreadItem::Plan { .. }
        | ThreadItem::Reasoning { .. }
        | ThreadItem::Sleep(_) => return None,
    };
    (!lines.is_empty()).then(|| PlainHistoryCell::new(lines))
}

fn command_execution_fallback_lines(
    command: &str,
    status: codex_app_server_protocol::CommandExecutionStatus,
    aggregated_output: Option<&str>,
    exit_code: Option<i32>,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = vec![vec!["$ ".dim(), command.to_string().into()].into()];
    lines.push(
        format!(
            "status: {status:?}{}",
            exit_code
                .map(|code| format!(" · exit {code}"))
                .unwrap_or_default()
        )
        .dim()
        .into(),
    );
    if let Some(output) = aggregated_output
        && !output.trim().is_empty()
    {
        lines.extend(
            output
                .lines()
                .map(|line| vec!["  ".dim(), line.trim_end().to_string().dim()].into()),
        );
    }
    lines
}

#[cfg(test)]
#[path = "thread_transcript_tests.rs"]
mod tests;
