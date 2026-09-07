//! Ambient terminal pets configured from the /pets slash command.
//!
//! The TUI treats built-in and custom pets differently on purpose:
//! built-in pets are versioned application assets fetched on demand into a
//! managed CODEX_HOME cache, while custom pets remain entirely user-owned data
//! under `$CODEX_HOME/pets/<pet-id>/pet.json` or legacy avatar directories.
//!
//! This module owns the TUI-facing contracts around that split:
//! resolving a selected pet id, preparing frames for terminal image protocols,
//! rendering the ambient sprite and picker preview, and preserving enough
//! metadata for `/pets` to behave like a first-class configuration surface.
//! It prepares built-in assets before loading pets, but does not own config
//! persistence or popup orchestration; callers must persist the final selection
//! only after the load succeeds.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

mod ambient;
mod asset_pack;
mod catalog;
mod frames;
mod image_protocol;
mod model;
mod picker;
mod preview;
mod sixel;

use anyhow::Context;
use anyhow::Result;
use codex_http_client::RouteAwareClientPool;
use codex_utils_absolute_path::AbsolutePathBuf;

use crate::tui::FrameRequester;

pub(crate) use ambient::AmbientPet;
pub(crate) use ambient::AmbientPetDraw;
pub(crate) use ambient::PetNotificationKind;
#[cfg(test)]
pub(crate) use ambient::test_ambient_pet;
pub(crate) use asset_pack::builtin_spritesheet_path;
#[cfg(test)]
pub(crate) use asset_pack::write_test_pack;
#[cfg(test)]
pub(crate) use image_protocol::ImageProtocol;
pub(crate) use image_protocol::PetImageSupport;
#[cfg(test)]
pub(crate) use image_protocol::PetImageUnsupportedReason;
#[cfg(not(test))]
pub(crate) use image_protocol::detect_pet_image_support;
pub(crate) use picker::PET_PICKER_VIEW_ID;
pub(crate) use picker::build_pet_picker_params;
pub(crate) use preview::PetPickerPreviewState;

pub(crate) const DEFAULT_PET_ID: &str = "codex";
pub(crate) const DISABLED_PET_ID: &str = "disabled";

/// iTerm2 3.6+ supports Kitty's local-file transport without loading image bytes into Codex.
pub(crate) fn local_file_image_previews_supported() -> bool {
    matches!(
        image_protocol::detect_pet_image_support(),
        PetImageSupport::Supported(image_protocol::ImageProtocol::KittyLocalFile)
    )
}

/// Ensure that a selected built-in pet has a locally cached spritesheet.
///
/// Custom pets are intentionally a no-op here because their source of truth is
/// already local. Preparing this before loading keeps first-use preview and
/// persistence failures at the asset-fetch boundary rather than surfacing as
/// deeper image-loading errors.
async fn ensure_builtin_pack_for_pet(
    pet_id: &str,
    codex_home: &std::path::Path,
    http_client: &RouteAwareClientPool,
) -> Result<()> {
    if let Some(pet) = catalog::builtin_pet(pet_id) {
        asset_pack::ensure_builtin_pet(codex_home, pet, http_client).await?;
    }
    Ok(())
}

/// Prepare a pet's built-in assets and load its synchronous state off the runtime.
pub(crate) async fn load_pet_with_assets(
    pet_id: String,
    codex_home: AbsolutePathBuf,
    frame_requester: FrameRequester,
    animations_enabled: bool,
    http_client: &RouteAwareClientPool,
) -> Result<AmbientPet> {
    ensure_builtin_pack_for_pet(&pet_id, &codex_home, http_client).await?;
    tokio::task::spawn_blocking(move || {
        AmbientPet::load(
            Some(&pet_id),
            &codex_home,
            frame_requester,
            animations_enabled,
        )
    })
    .await
    .context("join pet load task")?
}

#[derive(Debug)]
pub(crate) enum PetImageRenderError {
    Terminal(std::io::Error),
    Asset(anyhow::Error),
}

impl std::fmt::Display for PetImageRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Terminal(err) => write!(f, "terminal image write failed: {err}"),
            Self::Asset(err) => write!(f, "pet image asset unavailable: {err}"),
        }
    }
}

impl std::error::Error for PetImageRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Terminal(err) => Some(err),
            Self::Asset(err) => Some(err.as_ref()),
        }
    }
}

impl From<std::io::Error> for PetImageRenderError {
    fn from(err: std::io::Error) -> Self {
        Self::Terminal(err)
    }
}

pub(crate) fn render_ambient_pet_image(
    writer: &mut impl Write,
    state: &mut PetImageRenderState,
    request: Option<AmbientPetDraw>,
) -> std::result::Result<(), PetImageRenderError> {
    render_pet_image(writer, state, /*image_id*/ 0xC0DE, request)
}

pub(crate) fn render_pet_picker_preview_image(
    writer: &mut impl Write,
    state: &mut PetImageRenderState,
    request: Option<AmbientPetDraw>,
) -> std::result::Result<(), PetImageRenderError> {
    render_pet_image(writer, state, /*image_id*/ 0xC0DF, request)
}

#[derive(Debug, Default)]
pub(crate) struct PetImageRenderState {
    last_sixel_clear_area: Option<SixelClearArea>,
    last_protocol: Option<image_protocol::ImageProtocol>,
}

/// A local-file image placement owned by the transcript workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalImagePreviewDraw {
    pub(crate) image_id: u32,
    pub(crate) path: std::path::PathBuf,
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) columns: u16,
    pub(crate) rows: u16,
}

#[derive(Debug)]
struct RenderedLocalImagePreview {
    request: LocalImagePreviewDraw,
    /// A downscaled PNG created for a non-PNG input. It is never retained after the preview
    /// stops being visible.
    temporary_path: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub(crate) struct LocalImagePreviewState {
    rendered: BTreeMap<u32, RenderedLocalImagePreview>,
}

impl Drop for LocalImagePreviewState {
    fn drop(&mut self) {
        for preview in self.rendered.values() {
            remove_temporary_preview(&preview.temporary_path);
        }
    }
}

const MAX_PREVIEW_SOURCE_PIXELS: u64 = 12_000_000;
const MAX_PREVIEW_DIMENSION: u32 = 1024;

fn remove_temporary_preview(path: &Option<PathBuf>) {
    if let Some(path) = path {
        let _ = fs::remove_file(path);
    }
}

/// Resolve an image to a PNG file without retaining image bytes in workspace state.
///
/// Kitty's `f=100` file transport accepts PNG only. PNG inputs stay zero-copy; other supported
/// formats are decoded once, bounded to a thumbnail-sized transient file, and deleted on cleanup.
fn preview_png_file(path: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    let reader =
        image::ImageReader::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = reader
        .with_guessed_format()
        .with_context(|| format!("identify {}", path.display()))?;
    if reader.format() == Some(image::ImageFormat::Png) {
        return Ok((path.to_path_buf(), None));
    }

    let (width, height) = reader
        .into_dimensions()
        .with_context(|| format!("read dimensions for {}", path.display()))?;
    let source_pixels = u64::from(width).saturating_mul(u64::from(height));
    if source_pixels > MAX_PREVIEW_SOURCE_PIXELS {
        anyhow::bail!(
            "image {} is too large for a terminal preview ({source_pixels} pixels)",
            path.display()
        );
    }

    let image = image::ImageReader::open(path)
        .with_context(|| format!("open {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("identify {}", path.display()))?
        .decode()
        .with_context(|| format!("decode {}", path.display()))?;
    let thumbnail = image.thumbnail(MAX_PREVIEW_DIMENSION, MAX_PREVIEW_DIMENSION);
    let mut temporary = tempfile::Builder::new()
        .prefix("codex-tui-preview-")
        .suffix(".png")
        .tempfile()
        .context("create temporary terminal preview")?;
    thumbnail
        .write_to(&mut temporary, image::ImageFormat::Png)
        .context("encode temporary terminal preview")?;
    let temporary_path = temporary
        .into_temp_path()
        .keep()
        .map_err(|err| anyhow::Error::msg(format!("persist temporary terminal preview: {err}")))?;
    Ok((temporary_path.clone(), Some(temporary_path)))
}

/// Render only changed local-file previews and delete placements no longer visible.
pub(crate) fn render_local_image_previews(
    writer: &mut impl Write,
    state: &mut LocalImagePreviewState,
    requests: &[LocalImagePreviewDraw],
) -> std::result::Result<(), PetImageRenderError> {
    use crossterm::cursor::MoveTo;
    use crossterm::cursor::RestorePosition;
    use crossterm::cursor::SavePosition;
    use crossterm::queue;

    let requested = requests
        .iter()
        .cloned()
        .map(|request| (request.image_id, request))
        .collect::<BTreeMap<_, _>>();

    let removed = state
        .rendered
        .keys()
        .copied()
        .filter(|image_id| !requested.contains_key(image_id))
        .collect::<Vec<_>>();
    for image_id in removed {
        if state.rendered.contains_key(&image_id) {
            write!(writer, "{}", image_protocol::kitty_delete_image(image_id))?;
        }
        if let Some(previous) = state.rendered.remove(&image_id) {
            remove_temporary_preview(&previous.temporary_path);
        }
    }

    for request in requested.values() {
        if state
            .rendered
            .get(&request.image_id)
            .is_some_and(|previous| previous.request == *request)
        {
            continue;
        }
        let (transmitted_path, temporary_path) =
            preview_png_file(&request.path).map_err(PetImageRenderError::Asset)?;
        if state.rendered.contains_key(&request.image_id)
            && let Err(err) = write!(
                writer,
                "{}",
                image_protocol::kitty_delete_image(request.image_id)
            )
        {
            remove_temporary_preview(&temporary_path);
            return Err(err.into());
        }
        if let Some(previous) = state.rendered.remove(&request.image_id) {
            remove_temporary_preview(&previous.temporary_path);
        }
        let payload = image_protocol::kitty_transmit_png_file_with_id(
            &transmitted_path,
            request.columns,
            request.rows,
            Some(request.image_id),
        )
        .map_err(PetImageRenderError::Asset);
        let render_result = (|| -> std::result::Result<(), PetImageRenderError> {
            let payload = payload?;
            queue!(writer, SavePosition)?;
            queue!(writer, MoveTo(request.x, request.y))?;
            write!(writer, "{payload}")?;
            queue!(writer, RestorePosition)?;
            Ok(())
        })();
        if let Err(err) = render_result {
            remove_temporary_preview(&temporary_path);
            return Err(err);
        }
        state.rendered.insert(
            request.image_id,
            RenderedLocalImagePreview {
                request: request.clone(),
                temporary_path,
            },
        );
    }

    writer.flush()?;
    Ok(())
}

fn render_pet_image(
    writer: &mut impl Write,
    state: &mut PetImageRenderState,
    image_id: u32,
    request: Option<AmbientPetDraw>,
) -> std::result::Result<(), PetImageRenderError> {
    use crossterm::cursor::MoveTo;
    use crossterm::cursor::RestorePosition;
    use crossterm::cursor::SavePosition;
    use crossterm::queue;
    use image_protocol::ImageProtocol;

    let Some(request) = request else {
        if state.last_protocol.take().is_some_and(is_kitty_protocol) {
            write!(writer, "{}", image_protocol::kitty_delete_image(image_id))?;
        }
        if let Some(area) = state.last_sixel_clear_area.take() {
            queue!(writer, SavePosition)?;
            clear_sixel_area(writer, area)?;
            queue!(writer, RestorePosition)?;
        }
        writer.flush()?;
        return Ok(());
    };

    if state.last_protocol.take().is_some_and(is_kitty_protocol)
        || is_kitty_protocol(request.protocol)
    {
        write!(writer, "{}", image_protocol::kitty_delete_image(image_id))?;
    }
    state.last_protocol = Some(request.protocol);

    let payload = match request.protocol {
        ImageProtocol::Kitty => AmbientPetPayload::Text(
            image_protocol::kitty_transmit_png_with_id(
                &request.frame,
                request.columns,
                request.rows,
                Some(image_id),
            )
            .map_err(PetImageRenderError::Asset)?,
        ),
        ImageProtocol::KittyLocalFile => AmbientPetPayload::Text(
            image_protocol::kitty_transmit_png_file_with_id(
                &request.frame,
                request.columns,
                request.rows,
                Some(image_id),
            )
            .map_err(PetImageRenderError::Asset)?,
        ),
        ImageProtocol::Sixel => {
            let path =
                image_protocol::sixel_frame(&request.frame, &request.sixel_dir, request.height_px)
                    .map_err(PetImageRenderError::Asset)?;
            let sixel = std::fs::read(&path)
                .with_context(|| format!("read {}", path.display()))
                .map_err(PetImageRenderError::Asset)?;
            AmbientPetPayload::Bytes(sixel)
        }
    };

    queue!(writer, SavePosition)?;
    let current_sixel_clear_area = if matches!(request.protocol, ImageProtocol::Sixel) {
        Some(SixelClearArea::from(&request))
    } else {
        None
    };
    if let Some(previous_area) = state.last_sixel_clear_area.take()
        && Some(previous_area) != current_sixel_clear_area
    {
        clear_sixel_area(writer, previous_area)?;
    }
    if let Some(area) = current_sixel_clear_area {
        clear_sixel_area(writer, area)?;
        state.last_sixel_clear_area = Some(area);
    }
    queue!(writer, MoveTo(request.x, request.y))?;
    match payload {
        AmbientPetPayload::Text(payload) => write!(writer, "{payload}")?,
        AmbientPetPayload::Bytes(payload) => writer.write_all(&payload)?,
    }
    queue!(writer, RestorePosition)?;
    writer.flush()?;
    Ok(())
}

enum AmbientPetPayload {
    Text(String),
    Bytes(Vec<u8>),
}

fn is_kitty_protocol(protocol: image_protocol::ImageProtocol) -> bool {
    matches!(
        protocol,
        image_protocol::ImageProtocol::Kitty | image_protocol::ImageProtocol::KittyLocalFile
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SixelClearArea {
    x: u16,
    clear_top_y: u16,
    clear_bottom_y: u16,
    columns: u16,
}

impl From<&AmbientPetDraw> for SixelClearArea {
    fn from(request: &AmbientPetDraw) -> Self {
        Self {
            x: request.x,
            clear_top_y: request.clear_top_y,
            clear_bottom_y: request.y.saturating_add(request.rows),
            columns: request.columns,
        }
    }
}

fn clear_sixel_area(writer: &mut impl Write, area: SixelClearArea) -> std::io::Result<()> {
    use crossterm::cursor::MoveTo;
    use crossterm::queue;

    let blank = " ".repeat(area.columns.into());
    for row in area.clear_top_y..area.clear_bottom_y {
        queue!(writer, MoveTo(area.x, row))?;
        write!(writer, "{blank}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;
    use std::io;
    use std::path::PathBuf;

    use super::image_protocol::ImageProtocol;
    use super::*;

    #[test]
    fn ambient_pet_image_restores_cursor_after_drawing() {
        let dir = tempfile::tempdir().unwrap();
        let frame = dir.path().join("frame.png");
        std::fs::write(&frame, b"png").unwrap();
        let request = AmbientPetDraw {
            frame,
            protocol: ImageProtocol::Kitty,
            x: 2,
            y: 3,
            clear_top_y: 3,
            columns: 4,
            rows: 5,
            height_px: 75,
            sixel_dir: PathBuf::new(),
        };
        let mut output = Vec::new();
        let mut state = PetImageRenderState::default();

        render_ambient_pet_image(&mut output, &mut state, Some(request)).unwrap();

        let output = String::from_utf8(output).unwrap();
        let save = output.find("\x1b7").expect("saves cursor position");
        let move_to = output.find("\x1b[4;3H").expect("moves to pet position");
        let image = output.find("cG5n").expect("writes image payload");
        let restore = output.find("\x1b8").expect("restores cursor position");
        assert!(save < move_to);
        assert!(move_to < image);
        assert!(image < restore);
    }

    #[test]
    fn kitty_pet_image_clear_deletes_without_moving_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let frame = dir.path().join("frame.png");
        std::fs::write(&frame, b"png").unwrap();
        let request = AmbientPetDraw {
            frame,
            protocol: ImageProtocol::Kitty,
            x: 2,
            y: 3,
            clear_top_y: 3,
            columns: 4,
            rows: 5,
            height_px: 75,
            sixel_dir: PathBuf::new(),
        };
        let mut output = Vec::new();
        let mut state = PetImageRenderState::default();

        render_ambient_pet_image(&mut output, &mut state, Some(request)).unwrap();
        output.clear();
        render_ambient_pet_image(&mut output, &mut state, /*request*/ None).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Ga=d,d=I,i=49374,q=2;"));
        assert!(!output.contains("\x1b7"));
        assert!(!output.contains("\x1b["));
        assert!(!output.contains("\x1b8"));
    }

    #[test]
    fn kitty_local_file_pet_image_uses_file_reference_without_inline_payload() {
        let dir = tempfile::tempdir().unwrap();
        let frame = dir.path().join("frame.png");
        std::fs::write(&frame, b"png").unwrap();
        let request = AmbientPetDraw {
            frame,
            protocol: ImageProtocol::KittyLocalFile,
            x: 2,
            y: 3,
            clear_top_y: 3,
            columns: 4,
            rows: 2,
            height_px: 75,
            sixel_dir: PathBuf::new(),
        };
        let mut output = Vec::new();
        let mut state = PetImageRenderState::default();

        render_ambient_pet_image(&mut output, &mut state, Some(request)).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("a=d,d=I,i=49374,q=2;"));
        assert!(output.contains("\x1b[4;3H"));
        assert!(output.contains("a=T,t=f,f=100,c=4,r=2,q=2,i=49374;"));
        assert!(!output.contains("cG5n"));
        assert!(output.contains("\x1b8"));
    }

    #[test]
    fn local_input_preview_references_the_file_and_skips_unchanged_requests() {
        let dir = tempfile::tempdir().unwrap();
        let frame = dir.path().join("input.png");
        image::RgbaImage::new(4, 3).save(&frame).unwrap();
        let request = LocalImagePreviewDraw {
            image_id: 0xC100_0000,
            path: frame.clone(),
            x: 2,
            y: 3,
            columns: 16,
            rows: 6,
        };
        let mut output = Vec::new();
        let mut state = LocalImagePreviewState::default();

        render_local_image_previews(&mut output, &mut state, &[request.clone()]).unwrap();

        let first = String::from_utf8(output.clone()).unwrap();
        assert!(first.contains("a=T,t=f,f=100,c=16,r=6,q=2,i=3238002688;"));
        assert!(!first.contains("cG5n"));
        assert!(
            state
                .rendered
                .get(&0xC100_0000)
                .unwrap()
                .temporary_path
                .is_none()
        );

        output.clear();
        render_local_image_previews(&mut output, &mut state, &[request]).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn local_input_preview_converts_jpeg_and_removes_the_transient_png() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("input.jpg");
        image::RgbImage::new(4, 3)
            .save_with_format(&source, image::ImageFormat::Jpeg)
            .unwrap();
        let request = LocalImagePreviewDraw {
            image_id: 0xC100_0001,
            path: source.clone(),
            x: 2,
            y: 3,
            columns: 16,
            rows: 6,
        };
        let mut output = Vec::new();
        let mut state = LocalImagePreviewState::default();

        render_local_image_previews(&mut output, &mut state, &[request]).unwrap();

        let rendered = state.rendered.get(&0xC100_0001).unwrap();
        let temporary_path = rendered.temporary_path.clone().expect("temporary PNG path");
        assert_ne!(temporary_path, source);
        assert_eq!(
            image::ImageReader::open(&temporary_path)
                .unwrap()
                .with_guessed_format()
                .unwrap()
                .format(),
            Some(image::ImageFormat::Png)
        );

        render_local_image_previews(&mut output, &mut state, &[]).unwrap();
        assert!(!temporary_path.exists());
        assert!(state.rendered.is_empty());
    }

    #[test]
    fn local_input_preview_state_drop_removes_a_transient_png() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("input.jpg");
        image::RgbImage::new(4, 3)
            .save_with_format(&source, image::ImageFormat::Jpeg)
            .unwrap();
        let temporary_path = {
            let request = LocalImagePreviewDraw {
                image_id: 0xC100_0002,
                path: source,
                x: 2,
                y: 3,
                columns: 16,
                rows: 6,
            };
            let mut output = Vec::new();
            let mut state = LocalImagePreviewState::default();
            render_local_image_previews(&mut output, &mut state, &[request]).unwrap();
            state
                .rendered
                .get(&0xC100_0002)
                .unwrap()
                .temporary_path
                .clone()
                .expect("temporary PNG path")
        };

        assert!(!temporary_path.exists());
    }

    #[test]
    fn local_input_preview_write_failure_keeps_cleanup_state() {
        struct FailingWriter;

        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("injected preview write failure"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("input.jpg");
        image::RgbImage::new(4, 3)
            .save_with_format(&source, image::ImageFormat::Jpeg)
            .unwrap();
        let request = LocalImagePreviewDraw {
            image_id: 0xC100_0003,
            path: source,
            x: 2,
            y: 3,
            columns: 16,
            rows: 6,
        };
        let mut state = LocalImagePreviewState::default();
        render_local_image_previews(&mut Vec::new(), &mut state, &[request]).unwrap();
        let temporary_path = state
            .rendered
            .get(&0xC100_0003)
            .unwrap()
            .temporary_path
            .clone()
            .expect("temporary PNG path");

        assert!(render_local_image_previews(&mut FailingWriter, &mut state, &[]).is_err());
        assert!(state.rendered.contains_key(&0xC100_0003));
        drop(state);
        assert!(!temporary_path.exists());
    }

    #[test]
    fn sixel_pet_image_clears_cell_area_before_redrawing() {
        let dir = tempfile::tempdir().unwrap();
        let frame = dir.path().join("frame.png");
        std::fs::write(&frame, b"png").unwrap();
        let sixel_dir = dir.path().join("sixel");
        std::fs::create_dir(&sixel_dir).unwrap();
        let sixel_frame = sixel_dir.join("frame_h75_v2.six");
        std::fs::write(&sixel_frame, b"fake-sixel").unwrap();
        let request = AmbientPetDraw {
            frame,
            protocol: ImageProtocol::Sixel,
            x: 2,
            y: 3,
            clear_top_y: 1,
            columns: 4,
            rows: 2,
            height_px: 75,
            sixel_dir,
        };
        let mut output = Vec::new();
        let mut state = PetImageRenderState::default();

        render_ambient_pet_image(&mut output, &mut state, Some(request)).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\x1b[2;3H    \x1b[3;3H    \x1b[4;3H    \x1b[5;3H    \x1b[4;3H"));
        assert!(output.contains("fake-sixel"));
        assert!(output.contains("\x1b8"));
    }

    #[test]
    fn sixel_pet_image_clear_erases_last_drawn_area() {
        let dir = tempfile::tempdir().unwrap();
        let frame = dir.path().join("frame.png");
        std::fs::write(&frame, b"png").unwrap();
        let sixel_dir = dir.path().join("sixel");
        std::fs::create_dir(&sixel_dir).unwrap();
        let sixel_frame = sixel_dir.join("frame_h75_v2.six");
        std::fs::write(&sixel_frame, b"fake-sixel").unwrap();
        let request = AmbientPetDraw {
            frame,
            protocol: ImageProtocol::Sixel,
            x: 2,
            y: 3,
            clear_top_y: 1,
            columns: 4,
            rows: 2,
            height_px: 75,
            sixel_dir,
        };
        let mut output = Vec::new();
        let mut state = PetImageRenderState::default();

        render_ambient_pet_image(&mut output, &mut state, Some(request)).unwrap();
        output.clear();
        render_ambient_pet_image(&mut output, &mut state, /*request*/ None).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("Ga=d,d=I,i=49374,q=2;"));
        assert!(output.contains("\x1b7"));
        assert!(output.contains("\x1b[2;3H    \x1b[3;3H    \x1b[4;3H    \x1b[5;3H    "));
        assert!(output.contains("\x1b8"));
        assert!(!output.contains("fake-sixel"));
    }

    #[test]
    fn missing_frame_is_an_asset_error() {
        let dir = tempfile::tempdir().unwrap();
        let request = AmbientPetDraw {
            frame: dir.path().join("missing.png"),
            protocol: ImageProtocol::Kitty,
            x: 2,
            y: 3,
            clear_top_y: 3,
            columns: 4,
            rows: 5,
            height_px: 75,
            sixel_dir: PathBuf::new(),
        };
        let mut output = Vec::new();
        let mut state = PetImageRenderState::default();

        let err = render_ambient_pet_image(&mut output, &mut state, Some(request)).unwrap_err();

        assert!(matches!(err, PetImageRenderError::Asset(_)));
        assert!(err.source().is_some());
    }

    #[test]
    fn writer_failure_is_a_terminal_error() {
        struct FailingWriter;

        impl io::Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "test writer failed",
                ))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = FailingWriter;
        let mut state = PetImageRenderState {
            last_protocol: Some(ImageProtocol::Kitty),
            ..Default::default()
        };

        let err = render_ambient_pet_image(&mut writer, &mut state, /*request*/ None).unwrap_err();

        assert!(matches!(err, PetImageRenderError::Terminal(_)));
        assert!(err.source().is_some());
    }
}
