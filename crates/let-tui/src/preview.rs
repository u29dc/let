#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba};
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui_image::Resize as RatatuiResize;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;

const PREVIEW_ASPECT_WIDTH: u32 = 4;
const PREVIEW_ASPECT_HEIGHT: u32 = 3;
const DEFAULT_FONT_SIZE: (u16, u16) = (10, 20);
const DECODE_CACHE_CAPACITY: usize = 12;
const PREVIEW_BG: Rgba<u8> = Rgba([255, 255, 255, 255]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PreviewAssetKind {
    Photo,
    Floorplan,
    Satellite,
    Street,
}

impl PreviewAssetKind {
    fn label(self) -> &'static str {
        match self {
            Self::Photo => "photo",
            Self::Floorplan => "floorplan",
            Self::Satellite => "satellite",
            Self::Street => "street",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PreviewMode {
    Auto,
    Cover,
    Fit,
    Scale,
}

impl PreviewMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cover => "cover",
            Self::Fit => "fit",
            Self::Scale => "scale",
        }
    }

    pub(crate) fn cycle(self) -> Self {
        match self {
            Self::Auto => Self::Cover,
            Self::Cover => Self::Fit,
            Self::Fit => Self::Scale,
            Self::Scale => Self::Auto,
        }
    }

    fn resolve(self, kind: PreviewAssetKind) -> EffectivePreviewMode {
        match self {
            Self::Auto => match kind {
                PreviewAssetKind::Photo => EffectivePreviewMode::Cover,
                PreviewAssetKind::Floorplan
                | PreviewAssetKind::Satellite
                | PreviewAssetKind::Street => EffectivePreviewMode::Fit,
            },
            Self::Cover => EffectivePreviewMode::Cover,
            Self::Fit => EffectivePreviewMode::Fit,
            Self::Scale => EffectivePreviewMode::Scale,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EffectivePreviewMode {
    Cover,
    Fit,
    Scale,
}

impl EffectivePreviewMode {
    fn label(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Fit => "fit",
            Self::Scale => "scale",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PreviewTarget {
    pub(crate) path: PathBuf,
    pub(crate) kind: PreviewAssetKind,
    pub(crate) label: String,
}

impl PreviewTarget {
    pub(crate) fn new(path: PathBuf, kind: PreviewAssetKind, label: String) -> Self {
        Self { path, kind, label }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RequestKey {
    path: PathBuf,
    label: String,
    kind: PreviewAssetKind,
    mode: EffectivePreviewMode,
    user_mode: PreviewMode,
    width: u16,
    height: u16,
}

impl RequestKey {
    fn from_target(target: PreviewTarget, user_mode: PreviewMode, area: Rect) -> Self {
        let mode = user_mode.resolve(target.kind);
        Self {
            path: target.path,
            label: target.label,
            kind: target.kind,
            mode,
            user_mode,
            width: area.width,
            height: area.height,
        }
    }

    fn area(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }
}

enum PreviewState {
    Disabled(String),
    Empty(String),
    Loading(RequestKey),
    Failed {
        key: RequestKey,
        message: String,
    },
    Ready {
        key: RequestKey,
        protocol: Protocol,
        source_width: u32,
        source_height: u32,
    },
}

impl PreviewState {
    fn key(&self) -> Option<&RequestKey> {
        match self {
            Self::Loading(key) => Some(key),
            Self::Failed { key, .. } => Some(key),
            Self::Ready { key, .. } => Some(key),
            Self::Disabled(_) | Self::Empty(_) => None,
        }
    }
}

pub(crate) struct PreviewView<'a> {
    pub(crate) title: String,
    pub(crate) protocol: Option<&'a Protocol>,
    pub(crate) lines: Vec<Line<'static>>,
}

#[derive(Debug, Clone)]
struct PreviewRequest {
    id: u64,
    key: RequestKey,
    pixel_width: u32,
    pixel_height: u32,
}

#[derive(Debug)]
struct PreviewResponse {
    id: u64,
    key: RequestKey,
    result: Result<PreparedPreview, String>,
}

#[derive(Debug)]
struct PreparedPreview {
    image: DynamicImage,
    source_width: u32,
    source_height: u32,
}

#[derive(Clone)]
struct PreviewJobQueue {
    shared: Arc<(Mutex<PreviewJobSlot>, Condvar)>,
}

#[derive(Default)]
struct PreviewJobSlot {
    next: Option<PreviewRequest>,
    closed: bool,
}

impl PreviewJobQueue {
    fn new() -> Self {
        Self {
            shared: Arc::new((Mutex::new(PreviewJobSlot::default()), Condvar::new())),
        }
    }

    fn replace(&self, request: PreviewRequest) {
        let (lock, notify) = &*self.shared;
        let mut slot = lock.lock().expect("preview queue lock");
        slot.next = Some(request);
        notify.notify_one();
    }

    fn recv(&self) -> Option<PreviewRequest> {
        let (lock, notify) = &*self.shared;
        let mut slot = lock.lock().expect("preview queue lock");
        loop {
            if slot.closed {
                return None;
            }
            if let Some(request) = slot.next.take() {
                return Some(request);
            }
            slot = notify.wait(slot).expect("preview queue wait");
        }
    }

    fn close(&self) {
        let (lock, notify) = &*self.shared;
        let mut slot = lock.lock().expect("preview queue lock");
        slot.closed = true;
        slot.next = None;
        notify.notify_all();
    }
}

struct DecodeCache {
    entries: HashMap<PathBuf, DynamicImage>,
    order: VecDeque<PathBuf>,
    capacity: usize,
}

impl DecodeCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn load(&mut self, path: &Path) -> Result<&DynamicImage, String> {
        if self.entries.contains_key(path) {
            self.touch(path);
            return self
                .entries
                .get(path)
                .ok_or_else(|| "preview cache lookup failed".to_owned());
        }

        let image = image::open(path).map_err(|error| format!("decode failed: {error}"))?;
        self.entries.insert(path.to_path_buf(), image);
        self.touch(path);
        self.trim();
        self.entries
            .get(path)
            .ok_or_else(|| "preview cache insert failed".to_owned())
    }

    fn touch(&mut self, path: &Path) {
        self.order.retain(|entry| entry != path);
        self.order.push_back(path.to_path_buf());
    }

    fn trim(&mut self) {
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

pub(crate) struct PreviewController {
    picker: Option<Picker>,
    queue: Option<PreviewJobQueue>,
    response_rx: Option<Receiver<PreviewResponse>>,
    state: PreviewState,
    mode: PreviewMode,
    font_size: (u16, u16),
    request_id: u64,
}

impl PreviewController {
    pub(crate) fn detect() -> Self {
        match Picker::from_query_stdio() {
            Ok(mut picker) => {
                if should_force_ghostty_kitty() {
                    picker.set_protocol_type(ProtocolType::Kitty);
                }
                let font_size = picker.font_size();
                let queue = PreviewJobQueue::new();
                let worker_queue = queue.clone();
                let (response_tx, response_rx) = mpsc::channel();
                thread::Builder::new()
                    .name("let-tui-preview".to_owned())
                    .spawn(move || preview_worker_main(worker_queue, response_tx))
                    .expect("spawn preview worker");

                Self {
                    picker: Some(picker),
                    queue: Some(queue),
                    response_rx: Some(response_rx),
                    state: PreviewState::Empty("select a listing to preview".to_owned()),
                    mode: PreviewMode::Auto,
                    font_size,
                    request_id: 0,
                }
            }
            Err(error) => Self::disabled(format!("preview unavailable: {error}")),
        }
    }

    pub(crate) fn disabled(reason: impl Into<String>) -> Self {
        Self {
            picker: None,
            queue: None,
            response_rx: None,
            state: PreviewState::Disabled(reason.into()),
            mode: PreviewMode::Auto,
            font_size: DEFAULT_FONT_SIZE,
            request_id: 0,
        }
    }

    pub(crate) fn tick(&mut self) {
        let Some(response_rx) = self.response_rx.as_ref() else {
            return;
        };

        while let Ok(response) = response_rx.try_recv() {
            let current_key = self.state.key().cloned();
            if current_key.as_ref() != Some(&response.key) || response.id != self.request_id {
                continue;
            }

            let Some(picker) = self.picker.as_ref() else {
                self.state = PreviewState::Disabled("preview unavailable".to_owned());
                continue;
            };

            match response.result {
                Ok(prepared) => {
                    match picker.new_protocol(
                        prepared.image,
                        response.key.area(),
                        RatatuiResize::Scale(Some(FilterType::CatmullRom)),
                    ) {
                        Ok(protocol) => {
                            self.state = PreviewState::Ready {
                                key: response.key,
                                protocol,
                                source_width: prepared.source_width,
                                source_height: prepared.source_height,
                            };
                        }
                        Err(error) => {
                            self.state = PreviewState::Failed {
                                key: response.key,
                                message: format!("protocol build failed: {error}"),
                            };
                        }
                    }
                }
                Err(message) => {
                    self.state = PreviewState::Failed {
                        key: response.key,
                        message,
                    };
                }
            }
        }
    }

    pub(crate) fn preferred_block_height(&self, block_width: u16) -> u16 {
        if block_width <= 2 {
            return 0;
        }

        let inner_width = u32::from(block_width.saturating_sub(2));
        let pixel_width = inner_width.saturating_mul(u32::from(self.font_size.0));
        if pixel_width == 0 {
            return 0;
        }

        let pixel_height = pixel_width
            .saturating_mul(PREVIEW_ASPECT_HEIGHT)
            .div_ceil(PREVIEW_ASPECT_WIDTH);
        let inner_height = (pixel_height + u32::from(self.font_size.1).saturating_sub(1))
            / u32::from(self.font_size.1);
        (inner_height as u16).saturating_add(2)
    }

    pub(crate) fn mode(&self) -> PreviewMode {
        self.mode
    }

    pub(crate) fn cycle_mode(&mut self) {
        self.mode = self.mode.cycle();
    }

    pub(crate) fn has_pending_request(&self) -> bool {
        matches!(self.state, PreviewState::Loading(_))
    }

    pub(crate) fn sync(
        &mut self,
        target: Option<PreviewTarget>,
        area: Rect,
        empty_message: &'static str,
    ) {
        if matches!(self.state, PreviewState::Disabled(_)) {
            return;
        }

        let Some(target) = target else {
            self.state = PreviewState::Empty(empty_message.to_owned());
            return;
        };

        if area.width == 0 || area.height == 0 {
            self.state = PreviewState::Empty("preview area too small".to_owned());
            return;
        }

        let key = RequestKey::from_target(target, self.mode, area);
        if self.state.key() == Some(&key) {
            return;
        }

        let pixel_width = u32::from(area.width).saturating_mul(u32::from(self.font_size.0));
        let pixel_height = u32::from(area.height).saturating_mul(u32::from(self.font_size.1));
        self.request_id = self.request_id.wrapping_add(1);
        self.state = PreviewState::Loading(key.clone());

        let Some(queue) = self.queue.as_ref() else {
            self.state = PreviewState::Disabled("preview unavailable".to_owned());
            return;
        };

        queue.replace(PreviewRequest {
            id: self.request_id,
            key,
            pixel_width,
            pixel_height,
        });
    }

    pub(crate) fn view(&self) -> PreviewView<'_> {
        match &self.state {
            PreviewState::Disabled(message) => PreviewView {
                title: " preview unavailable ".to_owned(),
                protocol: None,
                lines: vec![Line::from("Preview disabled"), Line::from(message.clone())],
            },
            PreviewState::Empty(message) => PreviewView {
                title: format!(" preview {} ", self.mode.label()),
                protocol: None,
                lines: vec![Line::from(message.clone())],
            },
            PreviewState::Loading(key) => PreviewView {
                title: title_for_key(key),
                protocol: None,
                lines: vec![
                    Line::from(format!("loading {}", key.kind.label())),
                    Line::from(compact_path(&key.path)),
                    loading_mode_line(key),
                ],
            },
            PreviewState::Failed { key, message } => PreviewView {
                title: title_for_key(key),
                protocol: None,
                lines: vec![
                    Line::from(format!("failed to render {}", key.kind.label())),
                    Line::from(compact_path(&key.path)),
                    Line::from(message.clone()),
                ],
            },
            PreviewState::Ready {
                key,
                protocol,
                source_width,
                source_height,
            } => PreviewView {
                title: title_for_key(key),
                protocol: Some(protocol),
                lines: vec![
                    Line::from(format!("{}x{} source", source_width, source_height)),
                    loading_mode_line(key),
                ],
            },
        }
    }
}

impl Drop for PreviewController {
    fn drop(&mut self) {
        if let Some(queue) = &self.queue {
            queue.close();
        }
    }
}

fn title_for_key(key: &RequestKey) -> String {
    format!(
        " preview {} {} ",
        key.mode.label(),
        compact_label(&key.label, 18)
    )
}

fn loading_mode_line(key: &RequestKey) -> Line<'static> {
    let text = if key.user_mode == PreviewMode::Auto {
        format!("mode: {} -> {}", key.user_mode.label(), key.mode.label())
    } else {
        format!("mode: {}", key.mode.label())
    };
    Line::from(text)
}

fn preview_worker_main(queue: PreviewJobQueue, response_tx: mpsc::Sender<PreviewResponse>) {
    let mut cache = DecodeCache::new(DECODE_CACHE_CAPACITY);
    while let Some(request) = queue.recv() {
        let result = prepare_preview(&mut cache, &request);
        if response_tx
            .send(PreviewResponse {
                id: request.id,
                key: request.key,
                result,
            })
            .is_err()
        {
            break;
        }
    }
}

fn prepare_preview(
    cache: &mut DecodeCache,
    request: &PreviewRequest,
) -> Result<PreparedPreview, String> {
    if !request.key.path.exists() {
        return Err(format!("file not found: {}", request.key.path.display()));
    }

    let source = cache.load(&request.key.path)?;
    let (source_width, source_height) = source.dimensions();
    let image = match request.key.mode {
        EffectivePreviewMode::Cover => {
            normalize_cover(source, request.pixel_width, request.pixel_height)
        }
        EffectivePreviewMode::Fit => {
            normalize_contain(source, request.pixel_width, request.pixel_height, false)
        }
        EffectivePreviewMode::Scale => {
            normalize_contain(source, request.pixel_width, request.pixel_height, true)
        }
    };

    Ok(PreparedPreview {
        image,
        source_width,
        source_height,
    })
}

fn normalize_cover(image: &DynamicImage, target_w: u32, target_h: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            target_w.max(1),
            target_h.max(1),
            PREVIEW_BG,
        ));
    }

    let scale_w = target_w as f64 / width as f64;
    let scale_h = target_h as f64 / height as f64;
    let scale = scale_w.max(scale_h);
    let resized_w = ((width as f64) * scale).round().max(1.0) as u32;
    let resized_h = ((height as f64) * scale).round().max(1.0) as u32;

    let resized = image.resize_exact(resized_w, resized_h, FilterType::CatmullRom);
    let x = resized_w.saturating_sub(target_w) / 2;
    let y = resized_h.saturating_sub(target_h) / 2;
    resized.crop_imm(x, y, target_w.max(1), target_h.max(1))
}

fn normalize_contain(
    image: &DynamicImage,
    target_w: u32,
    target_h: u32,
    allow_upscale: bool,
) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            target_w.max(1),
            target_h.max(1),
            PREVIEW_BG,
        ));
    }

    let scale_w = target_w as f64 / width as f64;
    let scale_h = target_h as f64 / height as f64;
    let mut scale = scale_w.min(scale_h);
    if !allow_upscale {
        scale = scale.min(1.0);
    }

    let resized_w = ((width as f64) * scale).round().max(1.0) as u32;
    let resized_h = ((height as f64) * scale).round().max(1.0) as u32;
    let resized = image.resize_exact(resized_w, resized_h, FilterType::CatmullRom);

    let mut canvas = ImageBuffer::from_pixel(target_w.max(1), target_h.max(1), PREVIEW_BG);
    let offset_x = (target_w.saturating_sub(resized_w)) / 2;
    let offset_y = (target_h.saturating_sub(resized_h)) / 2;
    image::imageops::replace(
        &mut canvas,
        &resized.to_rgba8(),
        i64::from(offset_x),
        i64::from(offset_y),
    );
    DynamicImage::ImageRgba8(canvas)
}

fn compact_path(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn compact_label(label: &str, max: usize) -> String {
    if label.chars().count() <= max {
        return label.to_owned();
    }

    let mut out = String::with_capacity(max);
    for (index, ch) in label.chars().enumerate() {
        if index >= max.saturating_sub(1) {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

fn should_force_ghostty_kitty() -> bool {
    std::env::var("TERM_PROGRAM")
        .is_ok_and(|term_program| term_program.eq_ignore_ascii_case("ghostty"))
        || std::env::var("TERM").is_ok_and(|term| term.contains("ghostty"))
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, ImageBuffer, Rgba};

    use super::{
        PREVIEW_ASPECT_HEIGHT, PREVIEW_ASPECT_WIDTH, PreviewAssetKind, PreviewMode,
        normalize_contain, normalize_cover,
    };

    #[test]
    fn auto_mode_uses_cover_for_photos() {
        assert_eq!(
            PreviewMode::Auto.resolve(PreviewAssetKind::Photo).label(),
            "cover"
        );
        assert_eq!(
            PreviewMode::Auto
                .resolve(PreviewAssetKind::Floorplan)
                .label(),
            "fit"
        );
    }

    #[test]
    fn cover_resizes_to_exact_dimensions() {
        let input =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(400, 300, Rgba([0, 0, 0, 255])));
        let output = normalize_cover(&input, 1200, 900);
        assert_eq!(output.width(), 1200);
        assert_eq!(output.height(), 900);
    }

    #[test]
    fn contain_without_upscale_preserves_small_image() {
        let input =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(300, 400, Rgba([0, 0, 0, 255])));
        let output = normalize_contain(&input, 1200, 900, false);
        assert_eq!(output.width(), 1200);
        assert_eq!(output.height(), 900);
        assert_eq!(output.to_rgba8().get_pixel(0, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn contain_with_upscale_fills_more_canvas() {
        let input =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(100, 100, Rgba([0, 0, 0, 255])));
        let output = normalize_contain(&input, 400, 300, true);
        assert_eq!(output.width(), 400);
        assert_eq!(output.height(), 300);
        assert_eq!(PREVIEW_ASPECT_WIDTH, 4);
        assert_eq!(PREVIEW_ASPECT_HEIGHT, 3);
    }
}
