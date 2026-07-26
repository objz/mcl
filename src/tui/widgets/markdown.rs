use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use html_to_markdown_rs::{
    ConversionOptions, ImageMetadata,
    visitor::{HtmlVisitor, NodeContext, VisitResult, VisitorHandle},
};
use image::DynamicImage;
use pulldown_cmark::{Event, Options as MarkdownOptions, Parser, Tag, TagEnd};
use ratatui::{
    Frame,
    layout::{Rect, Size},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Wrap},
};
use ratatui_image::{
    Resize,
    picker::ProtocolType,
    sliced::{SignedPosition, SlicedImage, SlicedProtocol},
};
use the_other_tui_markdown::{RendererBuilder, Theme as MarkdownTheme, into_text_with_renderer};
use unicode_segmentation::UnicodeSegmentation;

use crate::config::settings::ImageProtocol;
use crate::config::theme::THEME;
use crate::instance::content::mods::{
    IconCell, fallback_icon, make_icon_pixels_from_image, make_icon_quadrants_from_image,
};

const MAX_DOCUMENT_WIDTH: u16 = 110;

struct TextBlock {
    source: String,
    rendered: Option<TextRender>,
}

struct TextRender {
    width: u16,
    text: Text<'static>,
    height: usize,
}

#[derive(Clone)]
struct ImageReference {
    url: String,
    alt: String,
    size: ImageSizeHint,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ImageSizeHint {
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ImageAlignment {
    #[default]
    Left,
    Center,
}

enum DocumentBlock {
    Text(TextBlock),
    ImageRow {
        images: Vec<ImageReference>,
        alignment: ImageAlignment,
    },
}

enum ImageLoad {
    Pending,
    Ready(DynamicImage),
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ImageRenderKey {
    width: u16,
    height: u16,
    protocol: ProtocolType,
    mode: ImageProtocol,
}

struct PreparedImage {
    protocol: Option<SlicedProtocol>,
    raster: Vec<Vec<IconCell>>,
}

struct PreparedImageResult {
    url: String,
    key: ImageRenderKey,
    result: Result<PreparedImage, String>,
}

struct DocumentImage {
    load: ImageLoad,
    prepared: Option<(ImageRenderKey, PreparedImage)>,
    pending: Option<ImageRenderKey>,
}

impl Default for DocumentImage {
    fn default() -> Self {
        Self {
            load: ImageLoad::Pending,
            prepared: None,
            pending: None,
        }
    }
}

pub struct Document {
    blocks: Vec<DocumentBlock>,
    images: HashMap<String, DocumentImage>,
    prepared_images: Arc<Mutex<Vec<PreparedImageResult>>>,
}

impl Document {
    pub fn new(title: &str, body: &str) -> Self {
        let normalized = normalize_html(body);
        let source = strip_duplicate_title(&normalized.markdown, title);
        let blocks = split_images(
            &source,
            &normalized.image_sizes,
            &normalized.image_alignments,
        );
        let images = blocks
            .iter()
            .flat_map(|block| match block {
                DocumentBlock::ImageRow { images, .. } => images.as_slice(),
                DocumentBlock::Text(_) => &[],
            })
            .map(|image| (image.url.clone(), DocumentImage::default()))
            .collect();
        Self {
            blocks,
            images,
            prepared_images: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn image_urls(&self) -> Vec<String> {
        self.images.keys().cloned().collect()
    }

    pub fn set_image(&mut self, url: &str, result: Result<DynamicImage, String>) {
        let Some(image) = self.images.get_mut(url) else {
            return;
        };
        image.load = match result {
            Ok(decoded) => ImageLoad::Ready(decoded),
            Err(error) => {
                tracing::debug!("Failed to load project image {url}: {error}");
                ImageLoad::Failed
            }
        };
        image.prepared = None;
        image.pending = None;
    }

    fn drain_prepared_images(&mut self) {
        let results = match self.prepared_images.lock() {
            Ok(mut pending) => std::mem::take(&mut *pending),
            Err(_) => return,
        };
        for result in results {
            let Some(image) = self.images.get_mut(&result.url) else {
                continue;
            };
            if image.pending != Some(result.key) {
                continue;
            }
            image.pending = None;
            match result.result {
                Ok(prepared) => image.prepared = Some((result.key, prepared)),
                Err(error) => {
                    tracing::debug!("Failed to prepare project image {}: {error}", result.url);
                }
            }
        }
    }
}

#[derive(Debug)]
struct FoundImage {
    range: Range<usize>,
    url: String,
    alt: String,
    alignment: ImageAlignment,
}

pub fn image_urls(title: &str, body: &str) -> Vec<String> {
    Document::new(title, body).image_urls()
}

pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage, String> {
    if let Ok(image) = image::load_from_memory(bytes) {
        return Ok(image);
    }
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|error| error.to_string())?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| "SVG image dimensions are too large".to_owned())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    let pixels = image::RgbaImage::from_raw(size.width(), size.height(), pixmap.take())
        .ok_or_else(|| "SVG renderer returned an invalid pixel buffer".to_owned())?;
    Ok(DynamicImage::ImageRgba8(pixels))
}

fn markdown_options() -> MarkdownOptions {
    MarkdownOptions::ENABLE_TABLES
        | MarkdownOptions::ENABLE_STRIKETHROUGH
        | MarkdownOptions::ENABLE_TASKLISTS
}

#[derive(Default)]
struct NormalizedDocument {
    markdown: String,
    image_sizes: HashMap<String, ImageSizeHint>,
    image_alignments: HashMap<String, ImageAlignment>,
}

fn normalize_html(source: &str) -> NormalizedDocument {
    let mut regions = Vec::new();
    let mut html_block_start = None;
    let mut inline_container = None;
    for (event, range) in Parser::new_ext(source, markdown_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::HtmlBlock) => html_block_start = Some(range.start),
            Event::End(TagEnd::HtmlBlock) => {
                if let Some(start) = html_block_start.take() {
                    regions.push(start..range.end);
                }
            }
            Event::Start(Tag::Paragraph | Tag::Heading { .. }) => {
                inline_container = Some((range.start, false));
            }
            Event::InlineHtml(_) => {
                if let Some((_, contains_html)) = inline_container.as_mut() {
                    *contains_html = true;
                }
            }
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_)) => {
                if let Some((start, true)) = inline_container.take() {
                    regions.push(start..range.end);
                }
            }
            _ => {}
        }
    }
    regions.sort_by_key(|range| range.start);
    regions.dedup();

    let mut normalized = source.to_owned();
    let mut image_sizes = HashMap::new();
    let mut image_alignments = HashMap::new();
    for range in regions.into_iter().rev() {
        let html = &source[range.clone()];
        match convert_html_fragment(html) {
            Ok(fragment) => {
                image_sizes.extend(fragment.image_sizes);
                image_alignments.extend(fragment.image_alignments);
                normalized.replace_range(range, &fragment.markdown);
            }
            Err(error) => {
                tracing::debug!("Failed to convert project HTML: {error}");
            }
        }
    }
    NormalizedDocument {
        markdown: normalized,
        image_sizes,
        image_alignments,
    }
}

fn convert_html_fragment(html: &str) -> Result<NormalizedDocument, String> {
    let image_alignments = Arc::new(Mutex::new(HashMap::new()));
    let visitor: VisitorHandle = Arc::new(Mutex::new(ImageAlignmentVisitor {
        centered: Vec::new(),
        image_alignments: image_alignments.clone(),
    }));
    let options = ConversionOptions {
        compact_tables: true,
        strip_tags: vec!["ins".to_owned(), "u".to_owned()],
        visitor: Some(visitor),
        ..ConversionOptions::default()
    };
    let result = html_to_markdown_rs::convert(&format!("<div>{html}</div>"), Some(options))
        .map_err(|error| error.to_string())?;
    let image_sizes = result
        .metadata
        .images
        .iter()
        .filter_map(image_size_hint)
        .collect();
    let image_alignments = image_alignments
        .lock()
        .map(|alignments| alignments.clone())
        .unwrap_or_default();
    Ok(NormalizedDocument {
        markdown: result.content.unwrap_or_default(),
        image_sizes,
        image_alignments,
    })
}

#[derive(Debug)]
struct ImageAlignmentVisitor {
    centered: Vec<bool>,
    image_alignments: Arc<Mutex<HashMap<String, ImageAlignment>>>,
}

impl HtmlVisitor for ImageAlignmentVisitor {
    fn visit_element_start(&mut self, context: &NodeContext<'_>) -> VisitResult {
        let attributes = context.attributes();
        let centered = self.centered.last().copied().unwrap_or_default()
            || context.tag_name.eq_ignore_ascii_case("center")
            || attributes
                .get("align")
                .is_some_and(|align| align.eq_ignore_ascii_case("center"))
            || attributes.get("style").is_some_and(|style| {
                style.split(';').any(|declaration| {
                    declaration.split_once(':').is_some_and(|(name, value)| {
                        name.trim().eq_ignore_ascii_case("text-align")
                            && value.trim().eq_ignore_ascii_case("center")
                    })
                })
            });
        self.centered.push(centered);
        VisitResult::Continue
    }

    fn visit_element_end(&mut self, _context: &NodeContext<'_>, _output: &str) -> VisitResult {
        self.centered.pop();
        VisitResult::Continue
    }

    fn visit_image(
        &mut self,
        _context: &NodeContext<'_>,
        source: &str,
        _alt: &str,
        _title: Option<&str>,
    ) -> VisitResult {
        if self.centered.last().copied().unwrap_or_default()
            && let Ok(mut alignments) = self.image_alignments.lock()
        {
            alignments.insert(source.to_owned(), ImageAlignment::Center);
        }
        VisitResult::Continue
    }
}

fn image_size_hint(image: &ImageMetadata) -> Option<(String, ImageSizeHint)> {
    let mut hint = image
        .dimensions
        .map(|dimensions| ImageSizeHint {
            width: Some(dimensions.width),
            height: Some(dimensions.height),
        })
        .unwrap_or_default();
    hint.width = hint.width.or_else(|| {
        image
            .attributes
            .get("width")
            .and_then(|value| pixel_size(value))
    });
    hint.height = hint.height.or_else(|| {
        image
            .attributes
            .get("height")
            .and_then(|value| pixel_size(value))
    });
    (hint != ImageSizeHint::default()).then(|| (image.src.clone(), hint))
}

fn pixel_size(value: &str) -> Option<u32> {
    let value = value.trim();
    let value = value.strip_suffix("px").unwrap_or(value).trim();
    (!value.is_empty() && value.chars().all(|character| character.is_ascii_digit()))
        .then(|| value.parse().ok())
        .flatten()
}

fn strip_duplicate_title(source: &str, title: &str) -> String {
    let mut heading_start = None;
    let mut heading_text = String::new();
    for (event, range) in Parser::new_ext(source, markdown_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. })
                if source[..range.start].trim().is_empty() && heading_start.is_none() =>
            {
                heading_start = Some(range.start);
            }
            Event::Text(text) | Event::Code(text) if heading_start.is_some() => {
                heading_text.push_str(&text);
            }
            Event::End(TagEnd::Heading(_)) if heading_start.is_some() => {
                if heading_text.trim().eq_ignore_ascii_case(title.trim()) {
                    return source[range.end..].trim_start_matches('\n').to_owned();
                }
                break;
            }
            Event::Start(_) if heading_start.is_none() => break,
            _ => {}
        }
    }
    source.to_owned()
}

fn split_images(
    source: &str,
    image_sizes: &HashMap<String, ImageSizeHint>,
    image_alignments: &HashMap<String, ImageAlignment>,
) -> Vec<DocumentBlock> {
    let mut images = Vec::<FoundImage>::new();
    let mut image_stack = Vec::<FoundImage>::new();
    let mut link_stack = Vec::<(usize, usize)>::new();

    for (event, range) in Parser::new_ext(source, markdown_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { .. }) => link_stack.push((range.start, images.len())),
            Event::End(TagEnd::Link) => {
                if let Some((start, first_image)) = link_stack.pop()
                    && first_image < images.len()
                {
                    images[first_image].range.start = start;
                    if let Some(last) = images.last_mut() {
                        last.range.end = range.end;
                    }
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                image_stack.push(FoundImage {
                    range: range.start..range.end,
                    url: dest_url.to_string(),
                    alt: String::new(),
                    alignment: image_alignments
                        .get(dest_url.as_ref())
                        .copied()
                        .unwrap_or(ImageAlignment::Left),
                });
            }
            Event::Text(text) if !image_stack.is_empty() => {
                if let Some(image) = image_stack.last_mut() {
                    image.alt.push_str(&text);
                }
            }
            Event::End(TagEnd::Image) => {
                if let Some(mut image) = image_stack.pop() {
                    image.range.end = range.end;
                    images.push(image);
                }
            }
            _ => {}
        }
    }

    images.sort_by_key(|image| image.range.start);
    let mut blocks = Vec::new();
    let mut image_row = Vec::new();
    let mut cursor = 0;
    for image in images {
        if image.range.start < cursor
            || image.range.end < image.range.start
            || image.range.end > source.len()
            || !source.is_char_boundary(image.range.start)
            || !source.is_char_boundary(image.range.end)
        {
            tracing::debug!("Ignoring invalid project image source range");
            continue;
        }
        let between = &source[cursor..image.range.start];
        let stays_in_row = !image_row.is_empty()
            && between.trim().is_empty()
            && !between.contains("\n\n")
            && image_row
                .first()
                .is_some_and(|(_, alignment)| *alignment == image.alignment);
        if !stays_in_row {
            push_image_row(&mut blocks, &mut image_row);
            push_text_block(&mut blocks, between);
        }
        image_row.push((
            ImageReference {
                size: image_sizes.get(&image.url).copied().unwrap_or_default(),
                url: image.url,
                alt: image.alt,
            },
            image.alignment,
        ));
        cursor = image.range.end;
    }
    push_image_row(&mut blocks, &mut image_row);
    if cursor < source.len() {
        push_text_block(&mut blocks, &source[cursor..]);
    }
    blocks
}

fn push_image_row(
    blocks: &mut Vec<DocumentBlock>,
    row: &mut Vec<(ImageReference, ImageAlignment)>,
) {
    if !row.is_empty() {
        let alignment = row[0].1;
        blocks.push(DocumentBlock::ImageRow {
            images: std::mem::take(row)
                .into_iter()
                .map(|(image, _)| image)
                .collect(),
            alignment,
        });
    }
}

fn push_text_block(blocks: &mut Vec<DocumentBlock>, source: &str) {
    let text = source.trim_matches('\n');
    if !text.trim().is_empty() {
        blocks.push(DocumentBlock::Text(TextBlock {
            source: format!("{text}\n"),
            rendered: None,
        }));
    }
}

fn formatted_text(source: &str, width: u16) -> Text<'static> {
    external_markdown_text(source, width)
}

fn external_markdown_text(source: &str, width: u16) -> Text<'static> {
    let theme = markdown_theme();
    let rule_style = theme.rule;
    let list_marker_style = theme.list_marker;
    let code_style = theme.code_block;
    let code_language_style = theme.code_block_lang;
    let renderer = RendererBuilder::new()
        .with_theme(theme)
        .with_heading(|_, spans| vec![Line::from(spans)])
        .with_rule(move || vec![Line::styled("─".repeat(usize::from(width)), rule_style)])
        .with_code_block(move |language, content| {
            code_block_lines(language, content, width, code_style, code_language_style)
        })
        .build();
    let mut text = into_text_with_renderer(source, &renderer);
    for line in &mut text.lines {
        line.spans.retain(|span| {
            let content = span.content.as_ref();
            content != "*"
                && !(span.style.add_modifier.contains(Modifier::UNDERLINED)
                    && content.starts_with("(http")
                    && content.ends_with(')'))
        });
        for span in &mut line.spans {
            if span.content.contains("**") {
                span.content = span.content.replace("**", "").into();
            }
        }
    }
    text.lines = text
        .lines
        .into_iter()
        .flat_map(|line| wrap_list_line(line, width, list_marker_style))
        .collect();
    text
}

fn code_block_lines(
    language: &str,
    content: &str,
    width: u16,
    code_style: Style,
    language_style: Style,
) -> Vec<Line<'static>> {
    let width = usize::from(width.max(1));
    let inner_width = width.saturating_sub(2).max(1);
    let mut lines = Vec::new();
    if !language.is_empty() {
        lines.push(padded_code_line(
            &format!("[{language}]"),
            width,
            language_style,
        ));
    }
    for source_line in content.strip_suffix('\n').unwrap_or(content).split('\n') {
        for line in split_display_width(source_line, inner_width) {
            lines.push(padded_code_line(&line, width, code_style));
        }
    }
    if lines.is_empty() {
        lines.push(Line::styled(" ".repeat(width), code_style));
    }
    lines
}

fn padded_code_line(content: &str, width: usize, style: Style) -> Line<'static> {
    let inner_width = width.saturating_sub(2);
    let content_width = Span::raw(content).width().min(inner_width);
    Line::styled(
        format!(
            " {content}{} ",
            " ".repeat(inner_width.saturating_sub(content_width))
        ),
        style,
    )
}

fn split_display_width(source: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut line_width = 0usize;
    for grapheme in source.graphemes(true) {
        let grapheme_width = Span::raw(grapheme).width();
        if line_width > 0 && line_width.saturating_add(grapheme_width) > width {
            lines.push(String::new());
            line_width = 0;
        }
        lines
            .last_mut()
            .expect("display line exists")
            .push_str(grapheme);
        line_width = line_width.saturating_add(grapheme_width);
    }
    lines
}

fn wrap_list_line(line: Line<'static>, width: u16, marker_style: Style) -> Vec<Line<'static>> {
    let Some(marker) = line.spans.first().filter(|span| span.style == marker_style) else {
        return vec![line];
    };
    let marker = format!("  {}", marker.content);
    let marker_width = Span::raw(&marker).width();
    let content_width = usize::from(width).saturating_sub(marker_width).max(1);
    let wrapped = wrap_styled_spans(&line.spans[1..], content_width);
    let continuation = " ".repeat(marker_width);
    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, mut spans)| {
            spans.insert(
                0,
                Span::styled(
                    if index == 0 {
                        marker.clone()
                    } else {
                        continuation.clone()
                    },
                    marker_style,
                ),
            );
            Line {
                style: line.style,
                alignment: line.alignment,
                spans,
            }
        })
        .collect()
}

fn wrap_styled_spans(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let mut lines = Vec::new();
    let mut line = Vec::new();
    let mut line_width = 0usize;
    let mut whitespace = Vec::new();
    let mut whitespace_width = 0usize;

    for span in spans {
        for part in span.content.split_word_bounds() {
            if part.chars().all(char::is_whitespace) {
                push_styled(&mut whitespace, part, span.style);
                whitespace_width = whitespace_width.saturating_add(Span::raw(part).width());
                continue;
            }

            let part_width = Span::raw(part).width();
            if line_width > 0
                && line_width
                    .saturating_add(whitespace_width)
                    .saturating_add(part_width)
                    > width
            {
                lines.push(std::mem::take(&mut line));
                line_width = 0;
                whitespace.clear();
                whitespace_width = 0;
            } else if line_width > 0 {
                line.append(&mut whitespace);
                line_width = line_width.saturating_add(whitespace_width);
                whitespace_width = 0;
            } else {
                whitespace.clear();
                whitespace_width = 0;
            }

            for grapheme in part.graphemes(true) {
                let grapheme_width = Span::raw(grapheme).width();
                if line_width > 0 && line_width.saturating_add(grapheme_width) > width {
                    lines.push(std::mem::take(&mut line));
                    line_width = 0;
                }
                push_styled(&mut line, grapheme, span.style);
                line_width = line_width.saturating_add(grapheme_width);
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

fn push_styled(spans: &mut Vec<Span<'static>>, content: &str, style: Style) {
    if let Some(last) = spans.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(content);
    } else {
        spans.push(Span::styled(content.to_owned(), style));
    }
}

fn markdown_theme() -> MarkdownTheme {
    let mut theme = MarkdownTheme::default();
    let heading = Style::default()
        .fg(THEME.as_ref().accent())
        .add_modifier(Modifier::BOLD);
    theme.base = Style::default().fg(THEME.as_ref().text());
    theme.h1 = heading;
    theme.h2 = heading;
    theme.h3 = heading;
    theme.h4 = heading;
    theme.h5 = heading;
    theme.h6 = heading;
    theme.inline_code = Style::default()
        .fg(THEME.as_ref().text())
        .bg(THEME.as_ref().surface());
    theme.code_block = theme.inline_code;
    theme.code_block_lang = Style::default()
        .fg(THEME.as_ref().text_dim())
        .bg(THEME.as_ref().surface());
    theme.link = Style::default()
        .fg(THEME.as_ref().info())
        .add_modifier(Modifier::UNDERLINED);
    theme.block_quote = Style::default().fg(THEME.as_ref().text_dim());
    theme.list_marker = Style::default().fg(THEME.as_ref().text_dim());
    theme.table_header = heading;
    theme.table_cell = Style::default().fg(THEME.as_ref().text());
    theme.table_separator = Style::default().fg(THEME.as_ref().border());
    theme.rule = theme.table_separator;
    theme.html = Style::default().fg(THEME.as_ref().text_dim());
    theme
}

impl TextBlock {
    fn prepare(&mut self, width: u16) -> &TextRender {
        if self
            .rendered
            .as_ref()
            .is_none_or(|rendered| rendered.width != width)
        {
            let text = formatted_text(&self.source, width);
            let height = Paragraph::new(text.clone())
                .wrap(Wrap { trim: false })
                .line_count(width)
                .max(1);
            self.rendered = Some(TextRender {
                width,
                text,
                height,
            });
        }
        self.rendered.as_ref().expect("text was prepared")
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    document: &mut Document,
    scroll: &mut usize,
    picker: &ratatui_image::picker::Picker,
) -> usize {
    if area.width == 0 || area.height == 0 {
        *scroll = 0;
        return 0;
    }
    let content_width = area.width.min(MAX_DOCUMENT_WIDTH);
    let content_area = Rect {
        x: area
            .x
            .saturating_add(area.width.saturating_sub(content_width) / 2),
        width: content_width,
        ..area
    };
    document.drain_prepared_images();
    let pending_preparations = document.prepared_images.clone();
    let (blocks, images) = (&mut document.blocks, &mut document.images);
    let mut heights = Vec::with_capacity(blocks.len());
    let mut image_rows = HashMap::new();
    for (index, block) in blocks.iter_mut().enumerate() {
        let height = match block {
            DocumentBlock::Text(text) => text.prepare(content_area.width).height,
            DocumentBlock::ImageRow {
                images: row,
                alignment,
            } => {
                let layout = image_row_layout(row, *alignment, images, content_area, picker);
                let height = usize::from(layout.height);
                image_rows.insert(index, layout);
                height
            }
        };
        heights.push(height);
    }

    let line_count = heights.iter().sum::<usize>();
    let max_scroll = line_count.saturating_sub(usize::from(area.height));
    *scroll = (*scroll).min(max_scroll);
    let viewport_start = *scroll;
    let viewport_end = viewport_start.saturating_add(usize::from(area.height));
    let mut document_y = 0usize;

    for (index, (block, height)) in blocks.iter_mut().zip(heights).enumerate() {
        let block_end = document_y.saturating_add(height);
        if block_end > viewport_start && document_y < viewport_end {
            let hidden_top = viewport_start.saturating_sub(document_y);
            let visible_y = document_y.saturating_sub(viewport_start);
            let visible_height = height
                .saturating_sub(hidden_top)
                .min(viewport_end.saturating_sub(document_y.max(viewport_start)));
            let block_area = Rect {
                x: content_area.x,
                y: area.y.saturating_add(visible_y as u16),
                width: content_area.width,
                height: visible_height as u16,
            };
            match block {
                DocumentBlock::Text(text) => {
                    let paragraph = Paragraph::new(text.prepare(content_area.width).text.clone())
                        .style(Style::default().fg(THEME.as_ref().text()))
                        .wrap(Wrap { trim: false })
                        .scroll((hidden_top.min(usize::from(u16::MAX)) as u16, 0));
                    frame.render_widget(paragraph, block_area);
                }
                DocumentBlock::ImageRow { images: row, .. } => {
                    if let Some(layout) = image_rows.get(&index) {
                        render_image_row(
                            frame,
                            block_area,
                            row,
                            layout,
                            images,
                            hidden_top,
                            picker,
                            &pending_preparations,
                        );
                    }
                }
            }
        }
        document_y = block_end;
    }

    if max_scroll > 0 {
        let scrollbar_position = (*scroll)
            .saturating_mul(line_count.saturating_sub(1))
            .checked_div(max_scroll)
            .unwrap_or_default();
        let mut scrollbar_state = ScrollbarState::new(line_count)
            .position(scrollbar_position)
            .viewport_content_length(usize::from(area.height));
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(None)
            .render(area, frame.buffer_mut(), &mut scrollbar_state);
    }
    max_scroll
}

struct ImageRowLayout {
    height: u16,
    width: u16,
    alignment: ImageAlignment,
    items: Vec<(u16, u16)>,
}

fn image_row_layout(
    row: &[ImageReference],
    alignment: ImageAlignment,
    images: &HashMap<String, DocumentImage>,
    area: Rect,
    picker: &ratatui_image::picker::Picker,
) -> ImageRowLayout {
    let gap_width = row.len().saturating_sub(1).min(usize::from(u16::MAX)) as u16;
    let available_width = area.width.saturating_sub(gap_width).max(1);
    let mut items = row
        .iter()
        .map(|reference| {
            images
                .get(&reference.url)
                .map_or((6.min(available_width), 3), |image| {
                    image_dimensions(image, reference.size, available_width, picker)
                })
        })
        .collect::<Vec<_>>();
    let total_width = items
        .iter()
        .map(|(width, _)| u32::from(*width))
        .sum::<u32>();
    if total_width > u32::from(available_width) {
        let scale = f64::from(available_width) / f64::from(total_width);
        for (width, height) in &mut items {
            *width = (f64::from(*width) * scale).floor().max(1.0) as u16;
            *height = (f64::from(*height) * scale).ceil().max(1.0) as u16;
        }
    }
    let width = items
        .iter()
        .map(|(width, _)| *width)
        .sum::<u16>()
        .saturating_add(gap_width);
    let height = items.iter().map(|(_, height)| *height).max().unwrap_or(1);
    ImageRowLayout {
        height,
        width,
        alignment,
        items,
    }
}

fn image_dimensions(
    image: &DocumentImage,
    hint: ImageSizeHint,
    available_width: u16,
    picker: &ratatui_image::picker::Picker,
) -> (u16, u16) {
    let ImageLoad::Ready(decoded) = &image.load else {
        return (6.min(available_width), 3);
    };
    let font = picker.font_size();
    let font_width = u32::from(font.width.max(1));
    let font_height = u32::from(font.height.max(1));
    let decoded_width = decoded.width().max(1);
    let decoded_height = decoded.height().max(1);
    let mut pixel_width = hint
        .width
        .or_else(|| {
            hint.height.map(|height| {
                height
                    .saturating_mul(decoded_width)
                    .div_ceil(decoded_height)
            })
        })
        .unwrap_or(decoded_width)
        .max(1);
    let mut pixel_height = hint
        .height
        .unwrap_or_else(|| {
            pixel_width
                .saturating_mul(decoded_height)
                .div_ceil(decoded_width)
        })
        .max(1);
    let available_pixels = u32::from(available_width.max(1)).saturating_mul(font_width);
    if pixel_width > available_pixels {
        pixel_height = pixel_height
            .saturating_mul(available_pixels)
            .div_ceil(pixel_width);
        pixel_width = available_pixels;
    }
    let width = pixel_width
        .div_ceil(font_width)
        .clamp(1, u32::from(available_width.max(1))) as u16;
    let height = pixel_height
        .div_ceil(font_height)
        .clamp(1, u32::from(u16::MAX)) as u16;
    (width, height)
}

#[allow(clippy::too_many_arguments)]
fn render_image_row(
    frame: &mut Frame,
    area: Rect,
    row: &[ImageReference],
    layout: &ImageRowLayout,
    images: &mut HashMap<String, DocumentImage>,
    hidden_top: usize,
    picker: &ratatui_image::picker::Picker,
    pending: &Arc<Mutex<Vec<PreparedImageResult>>>,
) {
    let row_start = match layout.alignment {
        ImageAlignment::Left => area.x,
        ImageAlignment::Center => area.x + area.width.saturating_sub(layout.width) / 2,
    };
    let mut x = row_start;
    for (reference, (width, height)) in row.iter().zip(&layout.items) {
        let item_y = usize::from(layout.height.saturating_sub(*height));
        let item_end = item_y.saturating_add(usize::from(*height));
        let viewport_start = hidden_top;
        let viewport_end = hidden_top.saturating_add(usize::from(area.height));
        if item_end > viewport_start && item_y < viewport_end {
            let item_hidden_top = viewport_start.saturating_sub(item_y);
            let visible_y = item_y.saturating_sub(viewport_start);
            let visible_height = item_end
                .min(viewport_end)
                .saturating_sub(item_y.max(viewport_start));
            let image_area = Rect {
                x,
                y: area.y.saturating_add(visible_y as u16),
                width: *width,
                height: visible_height as u16,
            };
            if let Some(image) = images.get_mut(&reference.url) {
                render_image(
                    frame,
                    image_area,
                    image,
                    reference,
                    item_hidden_top,
                    *height,
                    picker,
                    pending,
                );
            }
        }
        x = x.saturating_add(*width).saturating_add(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_image(
    frame: &mut Frame,
    area: Rect,
    image: &mut DocumentImage,
    reference: &ImageReference,
    hidden_top: usize,
    full_height: u16,
    picker: &ratatui_image::picker::Picker,
    pending: &Arc<Mutex<Vec<PreparedImageResult>>>,
) {
    let ImageLoad::Ready(decoded) = &image.load else {
        render_fallback(frame, area, hidden_top, &reference.alt);
        return;
    };
    let key = ImageRenderKey {
        width: area.width,
        height: full_height,
        protocol: picker.protocol_type(),
        mode: crate::config::SETTINGS.ui.image_protocol,
    };
    if image
        .prepared
        .as_ref()
        .is_none_or(|(prepared_key, _)| *prepared_key != key)
        && image.pending != Some(key)
    {
        image.pending = Some(key);
        prepare_image(
            reference.url.clone(),
            decoded.clone(),
            key,
            picker.clone(),
            pending.clone(),
        );
    }

    let Some((prepared_key, prepared)) = image.prepared.as_ref() else {
        render_fallback(frame, area, hidden_top, &reference.alt);
        return;
    };
    if *prepared_key != key {
        render_fallback(frame, area, hidden_top, &reference.alt);
        return;
    }

    if let Some(protocol) = prepared.protocol.as_ref() {
        let hidden_top = hidden_top.min(i16::MAX as usize) as i16;
        frame.render_widget(
            SlicedImage::new(protocol, SignedPosition::from((0, -hidden_top))),
            area,
        );
    } else {
        render_raster(frame, area, &prepared.raster, hidden_top);
    }
}

fn prepare_image(
    url: String,
    image: DynamicImage,
    key: ImageRenderKey,
    picker: ratatui_image::picker::Picker,
    pending: Arc<Mutex<Vec<PreparedImageResult>>>,
) {
    std::thread::spawn(move || {
        let result = prepare_image_render(image, key, &picker);
        if let Ok(mut pending) = pending.lock() {
            pending.push(PreparedImageResult { url, key, result });
        }
        crate::tui::request_redraw();
    });
}

fn prepare_image_render(
    image: DynamicImage,
    key: ImageRenderKey,
    picker: &ratatui_image::picker::Picker,
) -> Result<PreparedImage, String> {
    let raster = match key.mode {
        ImageProtocol::Quadrants if key.protocol == ProtocolType::Halfblocks => {
            make_icon_quadrants_from_image(&image, key.width, key.height)
        }
        _ => make_icon_pixels_from_image(&image, key.width, key.height),
    };
    let protocol = if key.protocol == ProtocolType::Halfblocks {
        None
    } else {
        let image = bottom_align_to_cells(image, key, picker);
        Some(
            SlicedProtocol::new_with_resize(
                picker,
                image,
                Size::new(key.width, key.height),
                Resize::Fit(None),
            )
            .map_err(|error| error.to_string())?,
        )
    };
    Ok(PreparedImage { protocol, raster })
}

fn bottom_align_to_cells(
    image: DynamicImage,
    key: ImageRenderKey,
    picker: &ratatui_image::picker::Picker,
) -> DynamicImage {
    let font = picker.font_size();
    let width = u32::from(key.width) * u32::from(font.width.max(1));
    let height = u32::from(key.height) * u32::from(font.height.max(1));
    let fitted = image.resize(width, height, image::imageops::FilterType::Lanczos3);
    if fitted.width() == width && fitted.height() == height {
        return fitted;
    }

    let mut canvas = image::RgbaImage::new(width, height);
    image::imageops::overlay(
        &mut canvas,
        &fitted,
        0,
        i64::from(height.saturating_sub(fitted.height())),
    );
    DynamicImage::ImageRgba8(canvas)
}

fn render_fallback(frame: &mut Frame, area: Rect, hidden_top: usize, alt: &str) {
    let fallback = fallback_icon();
    render_raster(frame, area, &fallback, hidden_top);
    if !alt.is_empty() && area.width > 8 {
        let label_area = Rect {
            x: area.x.saturating_add(7),
            width: area.width.saturating_sub(7),
            ..area
        };
        frame.render_widget(
            Paragraph::new(alt.to_owned()).style(Style::default().fg(THEME.as_ref().text_dim())),
            label_area,
        );
    }
}

fn render_raster(frame: &mut Frame, area: Rect, raster: &[Vec<IconCell>], hidden_top: usize) {
    for (visible_row, cells) in raster
        .iter()
        .skip(hidden_top)
        .take(usize::from(area.height))
        .enumerate()
    {
        let line = Line::from(
            cells
                .iter()
                .map(|cell| {
                    Span::styled(
                        cell.symbol.to_string(),
                        Style::default()
                            .fg(Color::Rgb(cell.fg_r, cell.fg_g, cell.fg_b))
                            .bg(Color::Rgb(cell.bg_r, cell.bg_g, cell.bg_b)),
                    )
                })
                .collect::<Vec<_>>(),
        );
        frame.render_widget(
            line,
            Rect {
                x: area.x,
                y: area.y.saturating_add(visible_row as u16),
                width: area.width,
                height: 1,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn html_images_are_extracted_and_raw_html_is_removed() {
        let document = super::Document::new(
            "Sodium",
            "<center><img src=\"https://cdn.example/comparison.png\" alt=\"Comparison\"></center>\n\n<h1>Installation</h1>",
        );
        assert_eq!(
            document.image_urls(),
            vec!["https://cdn.example/comparison.png"]
        );
        assert!(matches!(
            &document.blocks[0],
            super::DocumentBlock::ImageRow { alignment, .. }
                if *alignment == super::ImageAlignment::Center
        ));
        assert!(document.blocks.iter().any(
            |block| matches!(block, super::DocumentBlock::Text(text) if text.source.contains("Installation") && !text.source.contains("<h1>"))
        ));
    }

    #[test]
    fn inline_html_images_use_the_document_image_pipeline() {
        let document = super::Document::new(
            "Euphoria Patches",
            r#"<img src="https://cdn.example/logo.png" alt="Euphoria logo" width="30" /> A shader add-on"#,
        );
        assert_eq!(document.image_urls(), vec!["https://cdn.example/logo.png"]);
        assert!(matches!(
            &document.blocks[0],
            super::DocumentBlock::ImageRow { images, .. }
                if images[0].alt == "Euphoria logo"
        ));
        assert!(document.blocks.iter().any(
            |block| matches!(block, super::DocumentBlock::Text(text) if text.source.contains("A shader add-on") && !text.source.contains("<img"))
        ));
    }

    #[test]
    fn html_image_dimensions_are_kept_for_terminal_layout() {
        let document = super::Document::new(
            "Euphoria Patches",
            r#"<img src="https://cdn.example/logo.png" alt="Euphoria logo" width="30" height="auto" />"#,
        );
        assert!(matches!(
            &document.blocks[0],
            super::DocumentBlock::ImageRow { images, .. }
                if images[0].size == super::ImageSizeHint {
                    width: Some(30),
                    height: None,
                }
        ));
    }

    #[test]
    fn html_width_limits_the_rendered_terminal_image() {
        let image = super::DocumentImage {
            load: super::ImageLoad::Ready(image::DynamicImage::new_rgba8(300, 300)),
            prepared: None,
            pending: None,
        };
        let picker = ratatui_image::picker::Picker::halfblocks();
        assert_eq!(
            super::image_dimensions(
                &image,
                super::ImageSizeHint {
                    width: Some(30),
                    height: None,
                },
                120,
                &picker,
            ),
            (3, 2)
        );
    }

    #[test]
    fn inserted_html_keeps_its_content_and_hides_tag_shells() {
        let document = super::Document::new(
            "Project",
            "Support <ins>Reimagined</ins> and <ins>Unbound</ins>.</b>",
        );
        let source = document
            .blocks
            .iter()
            .find_map(|block| match block {
                super::DocumentBlock::Text(text) => Some(text.source.as_str()),
                _ => None,
            })
            .unwrap();
        let text = super::formatted_text(source, 80);
        let rendered = text
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .collect::<Vec<_>>();
        assert!(rendered.iter().all(|span| !span.content.contains('<')));
        assert!(
            ["Reimagined", "Unbound"]
                .iter()
                .all(|expected| rendered.iter().any(|span| span.content.contains(expected)))
        );
    }

    #[test]
    fn nested_inline_html_uses_dom_conversion_for_links_and_emphasis() {
        let document = super::Document::new(
            "Project",
            concat!(
                r#"<span class="badge"><a href="https://example.com">"#,
                "<strong>Supported</strong></a></span></strong></spanw>"
            ),
        );
        let source = document
            .blocks
            .iter()
            .find_map(|block| match block {
                super::DocumentBlock::Text(text) => Some(text.source.as_str()),
                _ => None,
            })
            .unwrap();
        assert!(source.contains("[**Supported**](https://example.com)"));
        assert!(!source.contains('<'));

        let text = super::formatted_text(source, 80);
        let supported = text
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .find(|span| span.content.contains("Supported"))
            .unwrap();
        assert!(
            supported
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        assert!(
            supported
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
        );
    }

    #[test]
    fn markdown_images_inside_links_do_not_leave_the_link_destination() {
        let document = super::Document::new(
            "Project",
            "[![Support](https://cdn.example/button.png)](https://example.com/donate)",
        );
        assert_eq!(document.blocks.len(), 1);
        assert!(matches!(
            &document.blocks[0],
            super::DocumentBlock::ImageRow { images, alignment }
                if images.len() == 1 && images[0].url == "https://cdn.example/button.png"
                    && *alignment == super::ImageAlignment::Left
        ));
    }

    #[test]
    fn visible_link_text_does_not_include_the_destination() {
        let text = super::formatted_text("[buying me a coffee](https://example.com/donate)", 80);
        let rendered = text
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(rendered, "buying me a coffee");
    }

    #[test]
    fn adjacent_images_share_a_row() {
        let document = super::Document::new(
            "Project",
            "[![Support](https://cdn.example/support.svg)](https://example.com) [![Chat](https://cdn.example/chat.svg)](https://example.com/chat)",
        );
        assert!(matches!(
            &document.blocks[0],
            super::DocumentBlock::ImageRow { images, .. } if images.len() == 2
        ));
    }

    #[test]
    fn explicitly_centered_html_images_share_a_centered_row() {
        let document = super::Document::new(
            "Project",
            r#"<p align="center"><a href="https://example.com"><img src="https://cdn.example/support.svg"></a> <img src="https://cdn.example/chat.svg"></p>"#,
        );
        assert!(matches!(
            &document.blocks[0],
            super::DocumentBlock::ImageRow { images, alignment }
                if images.len() == 2 && *alignment == super::ImageAlignment::Center
        ));
    }

    #[test]
    fn only_explicitly_centered_project_images_are_centered() {
        let document = super::Document::new(
            "Project",
            concat!(
                "![Badge](https://cdn.example/badge.png)\n\n",
                "<center><img src=\"https://cdn.example/comparison.png\"></center>\n\n",
                "![Loader](https://cdn.example/loader.png)"
            ),
        );
        let alignments = document
            .blocks
            .iter()
            .filter_map(|block| match block {
                super::DocumentBlock::ImageRow { alignment, .. } => Some(*alignment),
                super::DocumentBlock::Text(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            alignments,
            [
                super::ImageAlignment::Left,
                super::ImageAlignment::Center,
                super::ImageAlignment::Left
            ]
        );
    }

    #[test]
    fn headings_hide_markers_and_rules_use_terminal_lines() {
        let text = super::formatted_text("## Known Issues\n\n---", 12);
        let rendered = text
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|line| line == "Known Issues"));
        assert!(rendered.iter().any(|line| line == "────────────"));
        assert!(!rendered.iter().any(|line| line == "---"));
        assert!(!rendered.iter().any(|line| line.starts_with('#')));
    }

    #[test]
    fn all_heading_levels_are_bold() {
        let text = super::formatted_text("### Performance", 20);
        let heading = text
            .lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.contains("Performance"))
            })
            .unwrap();
        assert!(heading.spans.iter().any(|span| {
            span.style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        }));
    }

    #[test]
    fn matching_leading_project_title_is_removed_from_the_description() {
        let document = super::Document::new(
            "ImmediatelyFast",
            "# ImmediatelyFast\n\nImmediatelyFast is an open source Minecraft mod.",
        );
        let rendered = document
            .blocks
            .iter()
            .filter_map(|block| match block {
                super::DocumentBlock::Text(text) => Some(text.source.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(rendered.matches("ImmediatelyFast").count(), 1);
        assert!(!rendered.contains("# ImmediatelyFast"));
    }

    #[test]
    fn unrelated_and_later_headings_are_kept() {
        let document = super::Document::new(
            "ImmediatelyFast",
            "# Performance\n\nText\n\n## ImmediatelyFast",
        );
        let rendered = document
            .blocks
            .iter()
            .filter_map(|block| match block {
                super::DocumentBlock::Text(text) => Some(text.source.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert!(rendered.contains("# Performance"));
        assert!(rendered.contains("## ImmediatelyFast"));
    }

    #[test]
    fn lists_use_terminal_bullets() {
        let text = super::formatted_text("- first\n  - nested\n- second", 20);
        let rendered = text
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered, ["  • first", "    • nested", "  • second"]);
    }

    #[test]
    fn wrapped_list_items_keep_a_hanging_indent() {
        let text = super::formatted_text("- one two three four", 16);
        let rendered = text
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered, ["  • one two", "    three four"]);
        assert!(text.lines.iter().all(|line| line.width() <= 16));
    }

    #[test]
    fn markdown_headings_with_inline_html_are_converted_as_one_block() {
        let document = super::Document::new(
            "Project",
            r#"### Follow me? <a href="https://example.com"><span style="text-decoration: underline;">example.com</span></a>"#,
        );
        let source = document
            .blocks
            .iter()
            .find_map(|block| match block {
                super::DocumentBlock::Text(text) => Some(text.source.as_str()),
                _ => None,
            })
            .unwrap();
        assert!(!source.contains('<'), "{source}");
        let rendered = super::formatted_text(source, 80)
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(rendered, "Follow me? example.com");
    }

    #[test]
    fn html_lists_are_converted_as_one_document() {
        let document = super::Document::new(
            "Project",
            "### Features\n<ul><li><strong>First feature</strong>.</li><li>Second feature.</li></ul>",
        );
        let source = document
            .blocks
            .iter()
            .filter_map(|block| match block {
                super::DocumentBlock::Text(text) => Some(text.source.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert!(source.contains("- **First feature**."));
        assert!(source.contains("- Second feature."));
    }

    #[test]
    fn emphasis_survives_html_list_conversion_without_literal_markers() {
        let document = super::Document::new(
            "Project",
            "<ul><li>The effects are <strong>important</strong> and <em>optional</em>.</li></ul>",
        );
        let source = document
            .blocks
            .iter()
            .find_map(|block| match block {
                super::DocumentBlock::Text(text) => Some(text.source.as_str()),
                _ => None,
            })
            .unwrap();
        let text = super::formatted_text(source, 80);
        let rendered = text
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(!rendered.contains('*'));
        assert!(rendered.contains("important"));
        assert!(rendered.contains("optional"));
    }

    #[test]
    fn emphasis_next_to_html_tag_boundaries_does_not_leak_markers() {
        let document = super::Document::new(
            "Project",
            concat!(
                "<ul><li>The effects are <em>xaerominimap:no_cave_maps. ",
                "</em>The effects are neutral.</li>",
                "<li><strong>Hostile and friendly</strong> mobs can be colored ",
                "<strong>differently.</strong>Can also be displayed as icons.</li></ul>"
            ),
        );
        let source = document
            .blocks
            .iter()
            .find_map(|block| match block {
                super::DocumentBlock::Text(text) => Some(text.source.as_str()),
                _ => None,
            })
            .unwrap();
        let rendered = super::formatted_text(source, 100)
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(!rendered.contains('*'), "{rendered}");
    }

    #[test]
    fn fenced_code_blocks_are_rendered_without_fences() {
        let text = super::formatted_text("```json5\n{\n  \"enabled\": true\n}\n```", 24);
        let rendered = text
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|line| line.contains("\"enabled\": true"))
        );
        assert!(rendered.iter().all(|line| !line.contains("```")));
        let code_lines = text
            .lines
            .iter()
            .filter(|line| line.width() == 24)
            .collect::<Vec<_>>();
        assert_eq!(code_lines.len(), 4);
        assert!(
            code_lines.iter().all(|line| {
                line.style.bg == Some(crate::config::theme::THEME.as_ref().surface())
            })
        );
    }

    #[test]
    fn tables_are_rendered_by_the_external_renderer() {
        let text = super::formatted_text(
            "| Mod | Status | Note |\n| --- | --- | --- |\n| Player Animator | Supported | This means mods that use it like Better Combat and Emotecraft |",
            48,
        );
        assert!(
            text.lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.content.contains("Emotecraft"))
        );
    }

    #[test]
    fn svg_images_are_rasterized_for_terminal_protocols() {
        let image = super::decode_image(
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16">
                <rect width="32" height="16" fill="#48a23f"/>
            </svg>"##,
        )
        .unwrap();
        assert_eq!(image.width(), 32);
        assert_eq!(image.height(), 16);
    }

    #[test]
    fn gif_images_decode_their_first_frame() {
        let image = super::decode_image(&[
            0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xff, 0xff, 0xff, 0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00,
            0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
        ])
        .unwrap();
        assert_eq!(image.width(), 1);
        assert_eq!(image.height(), 1);
    }

    #[test]
    fn image_rows_align_fitted_images_to_the_bottom() {
        let picker = ratatui_image::picker::Picker::halfblocks();
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            3,
            image::Rgba([255, 255, 255, 255]),
        ));
        let aligned = super::bottom_align_to_cells(
            image,
            super::ImageRenderKey {
                width: 1,
                height: 2,
                protocol: picker.protocol_type(),
                mode: crate::config::settings::ImageProtocol::Halfblocks,
            },
            &picker,
        )
        .to_rgba8();

        assert_eq!(aligned.get_pixel(0, 0).0[3], 0);
        assert_eq!(aligned.get_pixel(0, aligned.height() - 1).0[3], 255);
    }

    #[test]
    fn markdown_rendering_clamps_scroll_to_the_document() {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut scroll = 0;
        let mut document = super::Document::new(
            "Project",
            "**Bold** text\n\n- one\n- two\n- three\n- four\n- five\n- six\n- seven\n- eight\n- nine\n- ten",
        );
        let picker = ratatui_image::picker::Picker::halfblocks();
        let mut max_scroll = 0;

        terminal
            .draw(|frame| {
                max_scroll =
                    super::render(frame, frame.area(), &mut document, &mut scroll, &picker);
            })
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Bold text"));

        scroll = usize::MAX;
        terminal
            .draw(|frame| {
                max_scroll =
                    super::render(frame, frame.area(), &mut document, &mut scroll, &picker);
            })
            .unwrap();
        assert_eq!(scroll, max_scroll);
        assert_eq!(terminal.backend().buffer()[(39, 7)].symbol(), "█");
    }

    #[test]
    fn multiple_images_inside_one_link_do_not_overlap() {
        let document = super::Document::new(
            "Project",
            "[![First](https://cdn.example/first.png) ![Second](https://cdn.example/second.png)](https://example.com)",
        );
        let mut urls = document.image_urls();
        urls.sort();
        assert_eq!(
            urls,
            [
                "https://cdn.example/first.png",
                "https://cdn.example/second.png"
            ]
        );
    }
}
