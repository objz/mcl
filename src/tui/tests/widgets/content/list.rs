use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::instance::content::entry::{ContentEntry, WorldDetails, WorldGameMode};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Widget,
};

use super::{
    ContentListState, WatcherEventHandling, available_description_width, description_text_width,
    diff_directory, diff_event_paths, ellipsize, load_provider_metadata, read_dir_stems,
    right_aligned_footer_spans, square_icon_columns, title_suffix_spans, watcher_event_handling,
    world_descriptions, world_game_mode_color,
};

fn entry(name: &str) -> ContentEntry {
    ContentEntry {
        file_stem: name.to_lowercase(),
        name: name.to_owned(),
        source_slug: None,
        installed_path: None,
        provider_project: None,
        world_details: None,
        title_suffix: None,
        footer_label: None,
        description: String::new(),
        enabled: true,
        icon_bytes: None,
        provider_icon: false,
        provider_description: false,
        path: PathBuf::from(name.to_lowercase()),
        icon_lines: None,
    }
}

#[test]
fn world_cards_preview_up_to_three_datapacks() {
    let lines = world_descriptions(&WorldDetails {
        game_mode: None,
        last_played: None,
        minecraft_version: Some("1.21.1".to_owned()),
        size: Some("4.0 MB".to_owned()),
        datapacks: ["A", "B", "C", "D"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    });

    assert_eq!(
        lines,
        ["1.21.1  •  4.0 MB", "  • A", "  • B", "  • C", "  +1 more"]
    );
}

#[test]
fn toggling_a_selected_entry_renames_and_updates_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("example.jar");
    std::fs::write(&path, b"mod").unwrap();
    let mut content = entry("Example");
    content.path = path.clone();
    let mut state = ContentListState::default();
    state.entries.push(content);
    state.list_state.selected = Some(0);

    state.toggle_selected();

    assert!(!path.exists());
    assert_eq!(
        state.entries[0].path,
        temp.path().join("example.jar.disabled")
    );
    assert!(!state.entries[0].enabled);
}

#[test]
fn content_watcher_ignores_file_access_events() {
    assert_eq!(
        watcher_event_handling(&notify::EventKind::Access(notify::event::AccessKind::Any)),
        WatcherEventHandling::Ignore
    );
}

#[test]
fn content_watcher_handles_mutations_without_full_rescan() {
    assert_eq!(
        watcher_event_handling(&notify::EventKind::Modify(notify::event::ModifyKind::Any)),
        WatcherEventHandling::Paths
    );
    assert_eq!(
        watcher_event_handling(&notify::EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::Both
        ))),
        WatcherEventHandling::Rescan
    );
    assert_eq!(
        watcher_event_handling(&notify::EventKind::Any),
        WatcherEventHandling::Rescan
    );
}

#[test]
fn content_watcher_keeps_a_renamed_disabled_entry() {
    let temp = tempfile::tempdir().unwrap();
    let enabled = temp.path().join("example.jar");
    let disabled = temp.path().join("example.jar.disabled");
    std::fs::write(&enabled, b"mod").unwrap();
    let known = Arc::new(Mutex::new(read_dir_stems(temp.path(), ".jar")));

    std::fs::rename(enabled, &disabled).unwrap();
    let diff = diff_directory(temp.path(), ".jar", None, &known).unwrap();

    assert_eq!(diff.toggled, vec![("example".to_owned(), false, disabled)]);
    assert!(diff.removed.is_empty());
    assert!(diff.added.is_empty());
}

#[test]
fn pure_toggle_does_not_request_reconciliation() {
    let mut state = ContentListState::default();
    let mut content = entry("Example");
    content.path = PathBuf::from("mods/example.jar.disabled");
    content.enabled = false;
    state.entries.push(content);
    *state.watcher_diff.lock().unwrap() = Some(super::WatcherDiff {
        toggled: vec![(
            "example".to_owned(),
            false,
            PathBuf::from("mods/example.jar.disabled"),
        )],
        removed: Vec::new(),
        added: Vec::new(),
    });

    let update = state.drain_watcher();

    assert!(!update.requires_reconcile);
    assert_eq!(update.toggles.len(), 1);
    assert_eq!(
        update.toggles[0].old_path,
        PathBuf::from("mods/example.jar")
    );
}

#[test]
fn irrelevant_watcher_paths_do_not_emit_an_empty_diff() {
    let temp = tempfile::tempdir().unwrap();
    let known = Arc::new(Mutex::new(HashMap::new()));
    let paths = vec![temp.path().join("notes.txt")];
    assert!(diff_event_paths(temp.path(), &paths, ".jar", None, &known).is_none());
}

#[test]
fn square_columns_follow_terminal_cell_ratio() {
    assert_eq!(square_icon_columns(3, (8, 16)), 6);
    assert_eq!(square_icon_columns(3, (8, 18)), 7);
    assert_eq!(square_icon_columns(6, (8, 18)), 14);
}

#[test]
fn square_columns_handle_missing_cell_size() {
    assert_eq!(square_icon_columns(3, (0, 0)), 3);
}

#[test]
fn title_badge_is_rendered_after_a_small_gap() {
    let label_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let spans = title_suffix_spans(Some("Installed"), Style::default(), label_style);
    let text = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<Vec<_>>()
        .concat();

    assert_eq!(text, "   Installed ");
    assert_eq!(spans[1].style, label_style);
    assert!(title_suffix_spans(None, Style::default(), label_style).is_empty());
}

#[test]
fn title_suffix_keeps_label_style_after_the_row_background_is_applied() {
    let label_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let text = Text::from(Line::from(title_suffix_spans(
        Some("downloads"),
        Style::default(),
        label_style,
    )))
    .style(Style::default().bg(Color::Black));
    let area = Rect::new(0, 0, 20, 1);
    let mut buffer = Buffer::empty(area);

    text.render(area, &mut buffer);

    let label_cell = buffer.cell((5, 0)).unwrap();
    assert_eq!(label_cell.fg, Color::Black);
    assert_eq!(label_cell.bg, Color::Cyan);
    assert!(label_cell.modifier.contains(Modifier::BOLD));
}

#[test]
fn world_modes_use_distinct_theme_roles() {
    let theme = crate::config::theme::THEME.as_ref();
    assert_eq!(
        world_game_mode_color(WorldGameMode::Survival),
        theme.success()
    );
    assert_eq!(world_game_mode_color(WorldGameMode::Creative), theme.info());
    assert_eq!(
        world_game_mode_color(WorldGameMode::Adventure),
        theme.warning()
    );
    assert_eq!(
        world_game_mode_color(WorldGameMode::Spectator),
        theme.text_dim()
    );
    assert_eq!(
        world_game_mode_color(WorldGameMode::Hardcore),
        theme.error()
    );
}

#[test]
fn descriptions_are_ellipsized_to_the_available_cell_width() {
    assert_eq!(ellipsize("short", 5), "short");
    assert_eq!(ellipsize("a longer description", 10), "a longe...");
    assert_eq!(ellipsize("narrow", 3), "...");
    assert_eq!(ellipsize("narrow", 2), "..");
    assert_eq!(ellipsize("界界界", 5), "界...");
}

#[test]
fn description_width_reserves_the_row_chrome() {
    assert_eq!(available_description_width(100, 6, true), 91);
    assert_eq!(available_description_width(100, 0, false), 98);
    assert_eq!(available_description_width(4, 6, true), 0);
}

#[test]
fn description_width_reserves_the_download_metadata() {
    assert_eq!(description_text_width(40, Some("1.2K downloads"), true), 25);
    assert_eq!(description_text_width(10, Some("1.2K downloads"), true), 0);
    assert_eq!(description_text_width(40, None, true), 40);
}

#[test]
fn footer_metadata_is_right_aligned_without_a_separator() {
    let mut spans = vec![Span::raw("Description")];
    spans.extend(right_aligned_footer_spans(
        30,
        "Description",
        true,
        "1.2K downloads",
        Style::default(),
    ));
    let line = Line::from(spans);

    assert_eq!(line.width(), 30);
    assert_eq!(line.to_string(), "Description     1.2K downloads");
}

#[test]
fn content_stream_inserts_entries_and_icons_incrementally() {
    let mut state = ContentListState::default();
    let stream = state.start_stream("remote");

    assert!(stream.send(entry("Zulu")));
    state.drain_pending();
    assert_eq!(state.entries[0].name, "Zulu");
    assert!(!state.loading);

    assert!(stream.send(entry("Alpha")));
    assert!(stream.send_icon("alpha".to_owned(), PathBuf::from("alpha"), vec![1, 2, 3],));
    state.drain_pending();

    assert_eq!(
        state
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["Alpha", "Zulu"]
    );
    assert_eq!(
        state.entries[0].icon_bytes.as_deref(),
        Some([1, 2, 3].as_slice())
    );
}

#[test]
fn source_stream_preserves_remote_result_order() {
    let mut state = ContentListState::default();
    let stream = state.start_source_stream("remote");
    assert!(stream.send(entry("Zulu")));
    assert!(stream.send(entry("Alpha")));

    state.drain_pending();

    assert_eq!(
        state
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["Zulu", "Alpha"]
    );
}

#[test]
fn source_refresh_reconciles_without_rebuilding_unchanged_entries() {
    let mut state = ContentListState::default();
    let initial = state.start_source_stream("remote");
    let mut alpha = entry("Alpha");
    alpha.icon_bytes = Some(vec![1, 2, 3]);
    assert!(initial.upsert(alpha));
    assert!(initial.upsert(entry("Beta")));
    assert!(initial.upsert(entry("Gamma")));
    state.drain_pending();
    state.list_state.selected = Some(0);

    let refresh = state.refresh_source_stream("remote");
    let mut alpha_update = entry("Alpha");
    alpha_update.description = "Updated".to_owned();
    assert!(refresh.upsert(alpha_update));
    assert!(refresh.upsert(entry("Delta")));
    assert!(refresh.retain(HashSet::from(["alpha".to_owned(), "delta".to_owned(),])));
    state.drain_pending();

    assert_eq!(
        state
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["Alpha", "Delta"]
    );
    assert_eq!(state.entries[0].description, "Updated");
    assert_eq!(
        state.entries[0].icon_bytes.as_deref(),
        Some([1, 2, 3].as_slice())
    );
    assert_eq!(state.list_state.selected, Some(0));
    assert!(!state.loading);
}

#[test]
fn provider_icons_are_requested_only_for_visible_missing_icons() {
    let mut state = ContentListState::default();
    let mut visible = entry("Visible");
    visible.icon_lines = Some(crate::instance::content::fallback_icon());
    visible.provider_project = Some(crate::instance::ProviderProject {
        provider: "modrinth".to_owned(),
        project_id: "visible-project".to_owned(),
        version_id: "version".to_owned(),
    });
    let mut offscreen = entry("Offscreen");
    offscreen.icon_lines = Some(crate::instance::content::fallback_icon());
    offscreen.provider_project = Some(crate::instance::ProviderProject {
        provider: "modrinth".to_owned(),
        project_id: "offscreen-project".to_owned(),
        version_id: "version".to_owned(),
    });
    state.entries = vec![visible, offscreen];
    state.rebuild_display_metadata();

    let projects = state.visible_provider_projects(&[0, 1], 3);

    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].project_id, "visible-project");
}

#[test]
fn complete_local_pack_metadata_does_not_request_provider_fallbacks() {
    let mut state = ContentListState::default();
    let mut visible = entry("Visible");
    visible.description = "Local description".to_owned();
    visible.icon_bytes = Some(vec![1, 2, 3]);
    visible.icon_lines = Some(crate::instance::content::fallback_icon());
    visible.provider_project = Some(crate::instance::ProviderProject {
        provider: "modrinth".to_owned(),
        project_id: "visible-project".to_owned(),
        version_id: "version".to_owned(),
    });
    state.entries = vec![visible];
    state.rebuild_display_metadata();

    assert!(state.visible_provider_projects(&[0], 3).is_empty());
}

#[tokio::test]
async fn streamed_entries_wait_for_their_rendered_icon() {
    let mut state = ContentListState::default();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(1, 1)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let mut with_icon = entry("With icon");
    with_icon.icon_bytes = Some(png.into_inner());
    let stream = state.start_stream("local");

    assert!(stream.send(with_icon));
    state.drain_pending();
    assert!(state.filtered_indices().is_empty());

    let picker = ratatui_image::picker::Picker::halfblocks();
    state.request_image_loads(&picker);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            state.drain_image_loads(&picker);
            if !state.filtered_indices().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("icon render completed");

    assert_eq!(state.filtered_indices(), vec![0]);
}

#[test]
fn streamed_entries_without_icons_are_visible_immediately() {
    let mut state = ContentListState::default();
    let stream = state.start_stream("local");

    assert!(stream.send(entry("Without icon")));
    state.drain_pending();

    assert_eq!(state.filtered_indices(), vec![0]);
}

#[test]
fn rendering_visible_entries_restores_the_first_selection() {
    let mut state = ContentListState::default();
    state.entries.push(entry("First"));
    state.rebuild_display_metadata();
    let picker = ratatui_image::picker::Picker::halfblocks();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 5)).unwrap();

    terminal
        .draw(|frame| {
            super::render(
                frame,
                frame.area(),
                &mut state,
                true,
                "Loading...",
                "Empty",
                &picker,
                false,
                false,
            );
        })
        .unwrap();

    assert_eq!(state.list_state.selected, Some(0));
}

#[test]
fn multiline_rendering_uses_the_space_beside_large_icons() {
    let mut world = entry("World");
    world.world_details = Some(WorldDetails {
        game_mode: Some(WorldGameMode::Survival),
        last_played: None,
        minecraft_version: Some("1.21.1".to_owned()),
        size: Some("2.0 MB".to_owned()),
        datapacks: Vec::new(),
    });
    world.icon_lines = Some(crate::instance::content::fallback_icon_large());
    let mut state = ContentListState::default();
    state.entries.push(world);
    state.rebuild_display_metadata();
    let picker = ratatui_image::picker::Picker::halfblocks();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(70, 6)).unwrap();

    terminal
        .draw(|frame| {
            super::render(
                frame,
                frame.area(),
                &mut state,
                true,
                "Loading...",
                "Empty",
                &picker,
                false,
                true,
            );
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend().to_string());
}

#[test]
fn pager_tracks_viewport_pages_and_jumps_to_their_first_item() {
    assert_eq!(
        super::pager_pages(0, 12),
        vec![Some(0), Some(1), Some(2), Some(3)]
    );
    assert_eq!(
        super::pager_pages(4, 12),
        vec![Some(0), None, Some(3), Some(4), Some(5)]
    );
    assert_eq!(
        super::pager_pages(11, 12),
        vec![Some(0), None, Some(9), Some(10), Some(11)]
    );

    let area = Rect::new(0, 0, 40, 13);
    let (list_area, pager) = super::pagination_layout(area, 5);
    assert_eq!(list_area, area);
    assert_eq!(pager.map(|(_, page_size)| page_size), Some(4));
    assert!(super::pagination_layout(area, 4).1.is_none());

    let mut state = ContentListState {
        entries: (1..=10)
            .map(|number| {
                let mut item = entry(&format!("Project {number}"));
                item.icon_lines = Some(crate::instance::content::fallback_icon());
                item
            })
            .collect(),
        ..ContentListState::default()
    };
    state.rebuild_display_metadata();
    let picker = ratatui_image::picker::Picker::halfblocks();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 13)).unwrap();

    terminal
        .draw(|frame| {
            super::render(
                frame,
                frame.area(),
                &mut state,
                true,
                "Loading...",
                "Empty",
                &picker,
                true,
                false,
            );
        })
        .unwrap();

    let pagination = state.pagination.as_ref().expect("pager");
    assert_eq!(pagination.page_size, 4);
    assert_eq!(pagination.page_count, 3);
    let page_two = pagination
        .hits
        .iter()
        .find(|(_, page)| *page == 1)
        .map(|(area, _)| (area.x, area.y))
        .expect("page two hit target");
    assert!(state.click_page(page_two.0, page_two.1));
    assert_eq!(state.list_state.selected, Some(4));
    assert!(state.next_page());
    assert_eq!(state.list_state.selected, Some(8));
    assert!(state.previous_page());
    assert_eq!(state.list_state.selected, Some(4));
}

#[test]
fn manifest_metadata_keeps_an_embedded_icon_renderer() {
    let minecraft_dir = PathBuf::from("instance/minecraft");
    let mut state = ContentListState::default();
    let mut installed = entry("Installed");
    installed.path = minecraft_dir.join("mods/installed.jar");
    installed.icon_bytes = Some(vec![1, 2, 3]);
    state.entries.push(installed);
    let picker = ratatui_image::picker::Picker::halfblocks();
    state.image_protocols.insert(
        "installed".to_owned(),
        picker.new_resize_protocol(image::DynamicImage::new_rgba8(1, 1)),
    );
    let mut manifest = crate::instance::ContentManifest::default();
    manifest.upsert(crate::instance::ContentFileRecord {
        relative_path: PathBuf::from("mods/installed.jar"),
        kind: crate::instance::ContentKind::Mod,
        enabled: true,
        fingerprint: crate::instance::FileFingerprint {
            size: 3,
            modified_ns: 1,
            hashes: Default::default(),
        },
        resolution: crate::instance::Resolution::Resolved {
            project: crate::instance::ProviderProject {
                provider: "modrinth".to_owned(),
                project_id: "project".to_owned(),
                version_id: "version".to_owned(),
            },
        },
        provider_aliases: Vec::new(),
        required_dependencies: Vec::new(),
        automatic_dependency: false,
        cleanup_eligible: false,
    });

    state.apply_manifest(&manifest, &minecraft_dir, crate::instance::ContentKind::Mod);

    assert!(state.image_protocols.contains_key("installed"));
}

#[test]
fn provider_metadata_fills_a_missing_installed_description() {
    let mut state = ContentListState::default();
    let mut installed = entry("Shader");
    installed.provider_project = Some(crate::instance::ProviderProject {
        provider: "modrinth".to_owned(),
        project_id: "shader-project".to_owned(),
        version_id: "version".to_owned(),
    });
    state.entries.push(installed);
    state
        .pending_provider_icons
        .lock()
        .unwrap()
        .push(super::PendingProviderIcon {
            provider: "modrinth".to_owned(),
            project_id: "shader-project".to_owned(),
            bytes: Vec::new(),
            description: "A cached shader description".to_owned(),
        });

    assert!(state.drain_provider_icons());
    assert_eq!(state.entries[0].description, "A cached shader description");
    assert!(state.entries[0].provider_description);
    assert_eq!(state.entries[0].title_suffix, None);
}

#[tokio::test]
async fn provider_metadata_loads_from_cache_without_network() {
    let temp = tempfile::tempdir().unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(1, 1)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let png = png.into_inner();
    let icon_path = crate::storage::MetadataPaths::new(temp.path())
        .provider_icons("modrinth")
        .join("cached-project.img");
    std::fs::create_dir_all(icon_path.parent().unwrap()).unwrap();
    std::fs::write(&icon_path, &png).unwrap();
    let project_path = crate::storage::MetadataPaths::new(temp.path())
        .provider_projects("modrinth")
        .join("cached-project.json");
    std::fs::create_dir_all(project_path.parent().unwrap()).unwrap();
    std::fs::write(
        project_path,
        serde_json::to_vec(&crate::net::modrinth::ProjectInfo {
            id: "cached-project".to_owned(),
            slug: "cached-project".to_owned(),
            title: "Cached project".to_owned(),
            description: "Cached description".to_owned(),
            body: String::new(),
            icon_url: None,
            categories: Vec::new(),
            additional_categories: Vec::new(),
            project_type: "mod".to_owned(),
            loaders: Vec::new(),
        })
        .unwrap(),
    )
    .unwrap();

    let installed = crate::instance::ProviderProject {
        provider: "modrinth".to_owned(),
        project_id: "cached-project".to_owned(),
        version_id: "cached-version".to_owned(),
    };
    let (bytes, description) =
        load_provider_metadata(&crate::net::HttpClient::new(), temp.path(), &installed)
            .await
            .unwrap();

    assert_eq!(bytes, png);
    assert_eq!(description, "Cached description");
}
