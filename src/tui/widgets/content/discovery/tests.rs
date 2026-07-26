use super::*;
use crate::tui::tests::UI_TEST_LOCK;
use chrono::Utc;
use crossterm::event::KeyModifiers;

fn instance(name: &str, version: &str) -> InstanceConfig {
    InstanceConfig {
        name: name.to_string(),
        game_version: version.to_string(),
        loader: ModLoader::Fabric,
        loader_version: None,
        created: Utc::now(),
        last_played: None,
        java_path: None,
        memory_max: None,
        memory_min: None,
        jvm_args: Vec::new(),
        resolution: None,
        config_sync_profile: None,
    }
}

fn version(id: &str) -> VersionInfo {
    VersionInfo {
        id: id.to_owned(),
        project_id: "project".to_owned(),
        name: format!("Version {id}"),
        version_number: id.to_owned(),
        game_versions: vec!["1.21.1".to_owned()],
        loaders: vec!["fabric".to_owned()],
        date_published: "2026-01-02T12:00:00Z".to_owned(),
        files: Vec::new(),
    }
}

#[test]
fn content_mode_toggles_both_ways() {
    assert_eq!(ContentMode::Installed.toggle(), ContentMode::Discover);
    assert_eq!(ContentMode::Discover.toggle(), ContentMode::Installed);
}

#[test]
fn project_metadata_is_split_between_title_and_footer_badges() {
    let entry = project_entry(
        DiscoveryProject {
            id: "example".to_owned(),
            slug: "example".to_owned(),
            title: "Example".to_owned(),
            description: "Project description".to_owned(),
            downloads: 1_234,
            icon_url: None,
            icon_bytes: None,
        },
        Some(PathBuf::from("example.jar")),
    );

    assert_eq!(entry.title_suffix.as_deref(), Some("Installed"));
    assert_eq!(entry.footer_label.as_deref(), Some("1.2K downloads"));
    assert_eq!(entry.description, "Project description");
    assert_eq!(entry.path, PathBuf::from("example"));
    assert_eq!(entry.installed_path, Some(PathBuf::from("example.jar")));
}

#[test]
fn install_and_change_version_popups_only_differ_in_title() {
    let project = DiscoveryProject {
        id: "project".to_owned(),
        slug: "project".to_owned(),
        title: "Project".to_owned(),
        description: String::new(),
        downloads: 0,
        icon_url: None,
        icon_bytes: None,
    };
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state
        .list
        .entries
        .push(project_entry(project.clone(), None));
    state.list.list_state.selected = Some(0);
    state.begin_versions().unwrap();
    assert_eq!(
        state.version_popup.as_ref().unwrap().title(),
        "Install Project"
    );

    state.version_popup = None;
    state.list.entries[0] = project_entry(project, Some(PathBuf::from("mods/project.jar")));
    state.begin_versions().unwrap();
    assert_eq!(
        state.version_popup.as_ref().unwrap().title(),
        "Change Project version"
    );
}

#[test]
fn compatible_versions_populate_the_open_popup() {
    let project = DiscoveryProject {
        id: "project".to_owned(),
        slug: "project".to_owned(),
        title: "Project".to_owned(),
        description: String::new(),
        downloads: 0,
        icon_url: None,
        icon_bytes: None,
    };
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state.list.entries.push(project_entry(project, None));
    state.list.list_state.selected = Some(0);
    let request = state.begin_versions().unwrap();
    DiscoveryState::push_action_result(
        &request.pending,
        DiscoveryActionResult::Versions {
            request_id: request.request_id,
            project_id: request.project_id,
            result: Ok(vec![version("1.0.0"), version("1.1.0")]),
        },
    );

    state.drain_pending();

    let popup = state.version_popup.as_ref().unwrap();
    assert!(!popup.loading);
    assert_eq!(popup.versions.len(), 2);
    assert_eq!(popup.selected, 0);
}

#[test]
fn project_page_loads_for_the_selected_discovery_entry() {
    let project = DiscoveryProject {
        id: "project".to_owned(),
        slug: "project".to_owned(),
        title: "Project".to_owned(),
        description: String::new(),
        downloads: 0,
        icon_url: None,
        icon_bytes: None,
    };
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state.list.entries.push(project_entry(project, None));
    state.list.list_state.selected = Some(0);

    let request = state.begin_project_page().unwrap();
    assert!(state.project_page_open());
    DiscoveryState::push_action_result(
        &request.pending,
        DiscoveryActionResult::ProjectPage {
            request_id: request.request_id,
            project_id: request.project_id,
            result: Ok(crate::net::modrinth::ProjectInfo {
                id: "project".to_owned(),
                slug: "project".to_owned(),
                title: "Project page".to_owned(),
                description: "Short description".to_owned(),
                body: "Long **Markdown** description.".to_owned(),
                icon_url: None,
            }),
        },
    );

    state.drain_pending();
    let page = state.project_page.as_ref().unwrap();
    assert_eq!(page.title, "Project page");
    assert!(page.document.is_some());
    state.project_page = None;
    assert!(state.begin_project_page().is_none());
    assert!(
        state
            .project_page
            .as_ref()
            .is_some_and(|page| page.document.is_some())
    );
}

#[test]
fn project_page_navigation_is_bounded_and_can_go_back() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state.project_page = Some(ProjectPageState {
        request_id: 1,
        project_id: "project".to_owned(),
        title: "Project".to_owned(),
        document: Some(crate::tui::widgets::markdown::Document::new(
            "Project", "Body",
        )),
        error: None,
        scroll: 0,
        max_scroll: 20,
    });

    handle_key(
        &KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        &mut state,
    );
    assert_eq!(state.project_page.as_ref().unwrap().scroll, 10);
    handle_key(
        &KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE),
        &mut state,
    );
    assert_eq!(state.project_page.as_ref().unwrap().scroll, 20);
    handle_key(
        &KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        &mut state,
    );
    assert!(!state.project_page_open());
}

#[test]
fn confirmation_can_return_to_version_selection() {
    let project = DiscoveryProject {
        id: "project".to_owned(),
        slug: "project".to_owned(),
        title: "Project".to_owned(),
        description: String::new(),
        downloads: 0,
        icon_url: None,
        icon_bytes: None,
    };
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state.list.entries.push(project_entry(project, None));
    state.list.list_state.selected = Some(0);
    let request = state.begin_versions().unwrap();
    DiscoveryState::push_action_result(
        &request.pending,
        DiscoveryActionResult::Versions {
            request_id: request.request_id,
            project_id: request.project_id,
            result: Ok(vec![version("1.0.0")]),
        },
    );
    state.drain_pending();
    assert!(state.begin_confirmation());

    handle_key(
        &KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        &mut state,
    );

    assert!(!state.version_popup.as_ref().unwrap().confirming);
    assert!(state.version_popup.is_some());
}

#[test]
fn discovery_delete_only_clears_the_matching_installed_badge() {
    let first_path = PathBuf::from("mods/first.jar");
    let second_path = PathBuf::from("mods/second.jar");
    let project = |id: &str| DiscoveryProject {
        id: id.to_owned(),
        slug: id.to_owned(),
        title: id.to_owned(),
        description: String::new(),
        downloads: 0,
        icon_url: None,
        icon_bytes: None,
    };
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state
        .list
        .entries
        .push(project_entry(project("first"), Some(first_path.clone())));
    state
        .list
        .entries
        .push(project_entry(project("second"), Some(second_path.clone())));
    state.list.list_state.selected = Some(0);

    let pending = state.pending_installed_delete().unwrap();
    assert_eq!(pending.path, first_path);
    assert!(state.clear_installed_path(&pending.path));

    assert_eq!(state.list.entries.len(), 2);
    assert!(state.list.entries[0].installed_path.is_none());
    assert!(state.list.entries[0].title_suffix.is_none());
    assert_eq!(
        state.list.entries[1].installed_path.as_deref(),
        Some(second_path.as_path())
    );
    assert_eq!(
        state.list.entries[1].title_suffix.as_deref(),
        Some("Installed")
    );
    assert!(state.pending_installed_delete().is_none());
}

#[test]
fn successful_install_marks_the_project_and_closes_the_popup() {
    let _guard = UI_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let project = DiscoveryProject {
        id: "project".to_owned(),
        slug: "project".to_owned(),
        title: "Project".to_owned(),
        description: String::new(),
        downloads: 0,
        icon_url: None,
        icon_bytes: None,
    };
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state.list.entries.push(project_entry(project, None));
    state.list.list_state.selected = Some(0);
    let versions_request = state.begin_versions().unwrap();
    DiscoveryState::push_action_result(
        &versions_request.pending,
        DiscoveryActionResult::Versions {
            request_id: versions_request.request_id,
            project_id: versions_request.project_id,
            result: Ok(vec![version("1.0.0")]),
        },
    );
    state.drain_pending();
    assert!(state.begin_confirmation());
    let install = state.begin_install().unwrap();
    assert!(state.version_popup.is_none());
    DiscoveryState::push_action_result(
        &install.pending,
        DiscoveryActionResult::Install {
            request_id: install.request_id,
            generation: install.generation,
            project_id: install.project_id,
            project_title: install.project_title,
            result: Ok(InstallCompletion {
                path: PathBuf::from("mods/project.jar"),
                replaced: false,
                skipped: false,
            }),
        },
    );

    state.drain_pending();
    assert!(state.version_popup.is_none());
    assert_eq!(
        state.list.entries[0].title_suffix.as_deref(),
        Some("Installed")
    );
    assert_eq!(
        state.list.entries[0].installed_path,
        Some(PathBuf::from("mods/project.jar"))
    );
    assert_eq!(state.list.entries[0].path, PathBuf::from("project"));
}

#[test]
fn installed_labels_follow_exact_manifest_projects() {
    let project = DiscoveryProject {
        id: "project-id".to_owned(),
        slug: "example-project".to_owned(),
        title: "Example Project".to_owned(),
        description: String::new(),
        downloads: 0,
        icon_url: None,
        icon_bytes: None,
    };
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state.list.entries.push(project_entry(project, None));

    let mut manifest = crate::instance::ContentManifest::default();
    manifest.upsert(crate::instance::ContentFileRecord {
        relative_path: PathBuf::from("mods/example-project-1.0.0.jar"),
        kind: crate::instance::ContentKind::Mod,
        enabled: true,
        fingerprint: crate::instance::FileFingerprint {
            size: 1,
            modified_ns: 1,
            hashes: Default::default(),
        },
        resolution: crate::instance::Resolution::Resolved {
            project: crate::instance::ProviderProject {
                provider: "modrinth".to_owned(),
                project_id: "project-id".to_owned(),
                version_id: "version".to_owned(),
            },
        },
    });
    state.refresh_installed_manifest(&manifest, std::path::Path::new("first"));
    assert_eq!(
        state.list.entries[0].title_suffix.as_deref(),
        Some("Installed")
    );
    assert_eq!(
        state.list.entries[0].installed_path,
        Some(PathBuf::from("first/mods/example-project-1.0.0.jar"))
    );

    state.refresh_installed_manifest(
        &crate::instance::ContentManifest::default(),
        std::path::Path::new("first"),
    );
    assert_eq!(state.list.entries[0].title_suffix, None);
    assert_eq!(state.list.entries[0].installed_path, None);
    assert_eq!(state.list.entries[0].path, PathBuf::from("project-id"));
}

#[test]
fn changing_instance_invalidates_results() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    let first = instance("one", "1.21.1");
    let second = instance("two", "1.21.1");
    let _request = state.begin_search(&first);

    assert!(!state.needs_search(&first));
    assert!(state.needs_search(&second));
}

#[test]
fn unavailable_vanilla_discovery_clears_cached_results() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state.list.entries.push(project_entry(
        DiscoveryProject {
            id: "cached".to_owned(),
            slug: "cached".to_owned(),
            title: "Cached".to_owned(),
            description: String::new(),
            downloads: 0,
            icon_url: None,
            icon_bytes: None,
        },
        None,
    ));
    let mut vanilla = instance("vanilla", "1.21.1");
    vanilla.loader = ModLoader::Vanilla;

    state.set_unavailable(&vanilla);

    assert!(state.list.entries.is_empty());
    assert!(!state.page_loading);
    assert!(state.exhausted);
}

#[test]
fn changing_instance_compatibility_invalidates_results() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    let original = instance("one", "1.21.1");
    let mut other_version = original.clone();
    other_version.game_version = "1.20.1".to_owned();
    let mut other_loader = original.clone();
    other_loader.loader = ModLoader::NeoForge;
    let _request = state.begin_search(&original);

    assert!(state.needs_search(&other_version));
    assert!(state.needs_search(&other_loader));
}

#[test]
fn stale_search_result_is_ignored() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    let instance = instance("one", "1.21.1");
    let old = state.begin_search(&instance);
    let _new = state.begin_search(&instance);
    DiscoveryState::push_result(
        &old.pending,
        old.generation,
        old.offset,
        Ok(DiscoveryPageResult {
            received: 20,
            total_hits: 99,
        }),
    );

    state.drain_pending();

    assert_eq!(state.total_hits, 0);
    assert!(state.list.loading);
}

#[test]
fn next_page_prefetches_before_selection_reaches_the_end() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state.set_viewport_rows(30);
    let instance = instance("one", "1.21.1");
    let first = state.begin_search(&instance);
    for index in 0..PAGE_SIZE {
        assert!(first.stream.send(project_entry(
            DiscoveryProject {
                id: index.to_string(),
                slug: index.to_string(),
                title: index.to_string(),
                description: String::new(),
                downloads: 0,
                icon_url: None,
                icon_bytes: None,
            },
            None
        )));
    }
    state.list.drain_pending();
    DiscoveryState::push_result(
        &first.pending,
        first.generation,
        first.offset,
        Ok(DiscoveryPageResult {
            received: PAGE_SIZE,
            total_hits: 300,
        }),
    );
    state.drain_pending();
    state.list.list_state.selected = Some(80);

    let next = state.begin_next_page().expect("next page should prefetch");
    assert_eq!(next.offset, PAGE_SIZE);
    assert!(state.begin_next_page().is_none());
}

#[test]
fn large_page_fills_a_tall_viewport_without_another_request() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    state.set_viewport_rows(90);
    let first = state.begin_search(&instance("one", "1.21.1"));
    for index in 0..PAGE_SIZE {
        assert!(first.stream.send(project_entry(
            DiscoveryProject {
                id: index.to_string(),
                slug: index.to_string(),
                title: index.to_string(),
                description: String::new(),
                downloads: 0,
                icon_url: None,
                icon_bytes: None,
            },
            None
        )));
    }
    state.list.drain_pending();
    DiscoveryState::push_result(
        &first.pending,
        first.generation,
        first.offset,
        Ok(DiscoveryPageResult {
            received: PAGE_SIZE,
            total_hits: 300,
        }),
    );
    state.drain_pending();

    assert!(state.begin_next_page().is_none());
}

#[test]
fn typing_keeps_loaded_results_until_remote_search_is_due() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    let request = state.begin_search(&instance("one", "1.21.1"));
    for title in ["Sodium", "Lithium"] {
        assert!(request.stream.send(project_entry(
            DiscoveryProject {
                id: title.to_lowercase(),
                slug: title.to_lowercase(),
                title: title.to_owned(),
                description: String::new(),
                downloads: 0,
                icon_url: None,
                icon_bytes: None,
            },
            None
        )));
    }
    state.list.drain_pending();

    handle_key(
        &KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        &mut state,
    );
    for character in "sod".chars() {
        handle_key(
            &KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
            &mut state,
        );
    }

    assert_eq!(state.list.filtered_indices(), vec![0, 1]);
    assert_eq!(state.list.search.query, "sod");
    assert!(!state.search_due());
    state.search_changed_at = Some(std::time::Instant::now() - SEARCH_DEBOUNCE);
    assert!(state.search_due());
}

#[test]
fn search_refresh_keeps_rows_until_the_diff_arrives() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    let instance = instance("one", "1.21.1");
    let initial = state.begin_search(&instance);
    for title in ["Sodium", "Lithium"] {
        assert!(initial.stream.upsert(project_entry(
            DiscoveryProject {
                id: title.to_lowercase(),
                slug: title.to_lowercase(),
                title: title.to_owned(),
                description: String::new(),
                downloads: 0,
                icon_url: None,
                icon_bytes: (title == "Sodium").then(|| vec![1, 2, 3]),
            },
            None
        )));
    }
    state.list.drain_pending();
    state.search.query = "sodium".to_owned();
    state.search_changed();

    let refresh = state.begin_search(&instance);

    assert!(refresh.reconcile);
    assert!(refresh.loaded_icon_stems.contains("sodium"));
    assert!(!refresh.loaded_icon_stems.contains("lithium"));
    assert_eq!(state.list.entries.len(), 2);
    assert!(!state.list.loading);
}

#[test]
fn pagination_continues_across_multiple_pages() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    let instance = instance("one", "1.21.1");
    let first = state.begin_search(&instance);
    for index in 0..PAGE_SIZE {
        assert!(first.stream.upsert(project_entry(
            DiscoveryProject {
                id: index.to_string(),
                slug: index.to_string(),
                title: index.to_string(),
                description: String::new(),
                downloads: 0,
                icon_url: None,
                icon_bytes: None,
            },
            None
        )));
    }
    state.list.drain_pending();
    DiscoveryState::push_result(
        &first.pending,
        first.generation,
        first.offset,
        Ok(DiscoveryPageResult {
            received: PAGE_SIZE,
            total_hits: 300,
        }),
    );
    state.drain_pending();
    state.list.list_state.selected = Some(PAGE_SIZE - MIN_PREFETCH_ITEMS);

    let second = state.begin_next_page().unwrap();
    for index in PAGE_SIZE..PAGE_SIZE * 2 {
        assert!(second.stream.upsert(project_entry(
            DiscoveryProject {
                id: index.to_string(),
                slug: index.to_string(),
                title: index.to_string(),
                description: String::new(),
                downloads: 0,
                icon_url: None,
                icon_bytes: None,
            },
            None
        )));
    }
    state.list.drain_pending();
    DiscoveryState::push_result(
        &second.pending,
        second.generation,
        second.offset,
        Ok(DiscoveryPageResult {
            received: PAGE_SIZE,
            total_hits: 300,
        }),
    );
    state.drain_pending();
    state.list.list_state.selected = Some(PAGE_SIZE * 2 - MIN_PREFETCH_ITEMS);

    let third = state.begin_next_page().unwrap();
    assert_eq!(third.offset, PAGE_SIZE * 2);
}

#[test]
fn permanent_pagination_failure_stops_without_discarding_loaded_entries() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    let first = state.begin_search(&instance("one", "1.21.1"));
    for index in 0..PAGE_SIZE {
        assert!(first.stream.upsert(project_entry(
            DiscoveryProject {
                id: index.to_string(),
                slug: index.to_string(),
                title: index.to_string(),
                description: String::new(),
                downloads: 0,
                icon_url: None,
                icon_bytes: None,
            },
            None
        )));
    }
    state.list.drain_pending();
    DiscoveryState::push_result(
        &first.pending,
        first.generation,
        first.offset,
        Ok(DiscoveryPageResult {
            received: PAGE_SIZE,
            total_hits: 300,
        }),
    );
    state.drain_pending();
    state.list.list_state.selected = Some(PAGE_SIZE - MIN_PREFETCH_ITEMS);
    let second = state.begin_next_page().unwrap();
    DiscoveryState::push_result(
        &second.pending,
        second.generation,
        second.offset,
        Err(DiscoveryPageError {
            message: "invalid response".to_owned(),
            retryable: false,
        }),
    );
    state.drain_pending();

    assert!(state.begin_next_page().is_none());
    assert_eq!(state.list.entries.len(), PAGE_SIZE);
    assert!(state.exhausted);
}

#[test]
fn transient_pagination_failure_retries_the_same_offset_after_a_delay() {
    let mut state = DiscoveryState::new(DiscoveryKind::Mod);
    let first = state.begin_search(&instance("one", "1.21.1"));
    DiscoveryState::push_result(
        &first.pending,
        first.generation,
        first.offset,
        Ok(DiscoveryPageResult {
            received: PAGE_SIZE,
            total_hits: 300,
        }),
    );
    state.drain_pending();

    let second = state.begin_next_page().unwrap();
    DiscoveryState::push_result(
        &second.pending,
        second.generation,
        second.offset,
        Err(DiscoveryPageError {
            message: "connection reset".to_owned(),
            retryable: true,
        }),
    );
    state.drain_pending();

    assert!(state.begin_next_page().is_none());
    state.retry_page_at = Some(std::time::Instant::now() - PAGE_RETRY_BASE_DELAY);
    assert_eq!(state.begin_next_page().unwrap().offset, PAGE_SIZE);
}
