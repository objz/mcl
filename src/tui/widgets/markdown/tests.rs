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
                && images[0].link.as_deref() == Some("https://example.com/donate")
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
        code_lines
            .iter()
            .all(|line| { line.style.bg == Some(crate::config::theme::THEME.as_ref().surface()) })
    );
}

#[test]
fn tables_wrap_cells_without_breaking_the_columns() {
    let text = super::formatted_text(
        "| Mod | Status | Note |\n| --- | --- | --- |\n| Player Animator | Supported | This means mods that use it like Better Combat and Emotecraft |",
        48,
    );
    assert!(text.lines.len() > 3);
    assert!(text.lines.iter().all(|line| line.width() <= 48));
    assert!(
        text.lines
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| span.content.contains("Emotecraft"))
    );
    let column_offsets = text
        .lines
        .iter()
        .filter_map(|line| {
            let mut offset = 0;
            let separators = line
                .spans
                .iter()
                .filter_map(|span| {
                    let start = offset;
                    offset += span.width();
                    span.content.contains('│').then_some(start)
                })
                .collect::<Vec<_>>();
            (!separators.is_empty()).then_some(separators)
        })
        .collect::<Vec<_>>();
    assert!(
        column_offsets.windows(2).all(|pair| pair[0] == pair[1]),
        "{column_offsets:?}\n{:#?}",
        text.lines
            .iter()
            .map(|line| line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>())
            .collect::<Vec<_>>()
    );
}

#[test]
fn rendered_links_keep_click_targets_after_wrapping() {
    let backend = TestBackend::new(24, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    let picker = ratatui_image::picker::Picker::halfblocks();
    let mut document = super::Document::new(
        "Project",
        "Open the [project documentation](https://example.com/docs) here.",
    );
    let mut scroll = 0;
    terminal
        .draw(|frame| {
            super::render(frame, frame.area(), &mut document, &mut scroll, &picker);
        })
        .unwrap();
    let hit = document.link_hits.first().unwrap();
    assert_eq!(
        document.link_at(hit.area.x, hit.area.y),
        Some("https://example.com/docs")
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
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xff, 0xff, 0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02,
        0x02, 0x44, 0x01, 0x00, 0x3b,
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
            max_scroll = super::render(frame, frame.area(), &mut document, &mut scroll, &picker);
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
            max_scroll = super::render(frame, frame.area(), &mut document, &mut scroll, &picker);
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
