# App path Windows baseline

Every test that failed on the Windows builder at `e03f087946`, 2026-11-18: 934 of the 13,831 that ran under
`cargo nextest run --workspace --exclude cmdr-fsevent-stream --no-fail-fast` (the macOS-only FSEvents crate has no
Windows build). Made by `scripts/windows-test-baseline` from the run's nextest JUnit report; each later app path
milestone diffs its run against this list with the same tool, so "turned green" and "newly red" are names, not counts.
Grouped by crate, then by cause cluster and the effort that owns it (`docs/specs/app-path-plan.md` § "Windows failures,
mapped"). "Cause unread" marks a failure whose output names no cause; it sits in its nearest sibling's cluster.

## cmdr (182)

### N, a native tempdir where an app path belongs: the app's file system and listings conversion (11)

- `file_system::watcher_incremental_test::a_batch_publishes_removes_then_adds_then_modifies`
- `file_system::watcher_incremental_test::a_child_renamed_away_leaves_the_listing`
- `file_system::watcher_incremental_test::a_full_re_read_has_landed_when_it_returns`
- `file_system::watcher_incremental_test::a_modify_keeps_the_finder_tags_the_cache_holds`
- `file_system::watcher_incremental_test::a_replaced_watch_root_falls_back_to_a_full_re_read`
- `file_system::watcher_incremental_test::a_rescan_falls_back_to_a_full_re_read`
- `file_system::watcher_incremental_test::over_500_events_fall_back_to_a_full_re_read`
- `file_system::watcher_incremental_test::root_metadata_and_grandchild_events_change_nothing`
- `file_system::watcher_test::a_dotfile_write_reaches_a_pane_showing_dotfiles`
- `file_system::watcher_test::a_dotfile_write_reaches_the_cache_but_not_a_pane_hiding_dotfiles`
- `network::smb_mount_gate::tests::a_lifted_requirement_lets_the_listing_through`

### N, a native tempdir where an app path belongs: the app's operations and tools conversion (56)

- `multi_rename::analytics_test::an_apply_reports_multi_rename_applied_from_the_engines_exit`
- `multi_rename::media_test::a_file_changing_under_a_fully_read_preview_makes_it_out_of_date_not_still_reading`
- `multi_rename::media_test::a_file_that_changed_since_it_was_read_reads_again`
- `multi_rename::media_test::a_file_that_fails_or_panics_counts_as_read_and_the_rest_go_on`
- `multi_rename::media_test::a_file_that_reads_resets_the_count_of_failures_in_a_row`
- `multi_rename::media_test::a_reader_that_reads_nothing_settles_every_file_unreadable_without_giving_up`
- `multi_rename::media_test::a_session_without_media_files_is_done_from_the_start`
- `multi_rename::media_test::a_volume_that_keeps_timing_out_is_given_up_on_after_three_files`
- `multi_rename::media_test::a_volume_that_went_away_reads_nothing_and_every_file_is_unreadable`
- `multi_rename::media_test::closing_the_sheet_stops_the_read_and_tells_the_read_in_flight`
- `multi_rename::media_test::every_media_file_is_read_once_and_progress_ends_with_done`
- `multi_rename::media_test::field_samples_say_reading_before_the_read_and_refuse_other_tokens`
- `multi_rename::media_test::field_samples_show_the_token_on_the_rows_asked_and_count_the_whole_session`
- `multi_rename::media_test::opening_counts_the_media_files_only`
- `multi_rename::media_test::rename_waits_while_a_row_is_still_reading`
- `multi_rename::media_test::the_first_file_goes_first_then_the_rows_on_screen_then_the_rest`
- `multi_rename::media_test::the_preview_uses_what_was_read`
- `multi_rename::results_test::a_name_typed_in_the_results_file_is_the_one_the_rename_uses`
- `multi_rename::results_test::a_results_file_the_user_deleted_reads_back_as_gone`
- `multi_rename::results_test::an_edit_read_back_after_the_preview_shown_never_sneaks_into_its_rename`
- `multi_rename::results_test::closing_the_sheet_removes_its_results_file`
- `multi_rename::results_test::reading_back_before_results_wrote_anything_is_refused`
- `multi_rename::results_test::the_desktop_offers_results_on_every_session`
- `multi_rename::results_test::using_the_settings_again_drops_the_edits_and_the_file`
- `multi_rename::run_test::a_batch_holding_a_dot_git_category_row_is_refused_as_read_only`
- `multi_rename::run_test::a_closed_session_answers_nothing`
- `multi_rename::run_test::a_file_appearing_above_the_selection_never_joins_it`
- `multi_rename::run_test::a_folder_that_changed_since_the_preview_is_refused`
- `multi_rename::run_test::a_preview_a_newer_one_replaced_is_refused`
- `multi_rename::run_test::a_row_renamed_onto_a_portal_name_is_refused_as_read_only`
- `multi_rename::run_test::a_selection_read_at_an_older_sequence_is_refused`
- `multi_rename::run_test::a_session_file_that_vanished_previews_as_missing`
- `multi_rename::run_test::normalizing_renames_a_decomposed_name_to_its_composed_spelling_on_disk`
- `multi_rename::run_test::nothing_ready_starts_nothing`
- `multi_rename::run_test::problems_only_pages_through_the_problem_rows_alone`
- `multi_rename::run_test::read_only_answers_before_not_connected`
- `multi_rename::run_test::the_folder_comes_out_without_diacritics_and_the_rest_is_left_alone`
- `multi_rename::run_test::the_preview_answers_one_page_and_the_rest_pages_in`
- `multi_rename::session_test::a_move_retires_the_preview_on_screen_so_apply_waits_for_the_next`
- `multi_rename::session_test::a_moved_row_takes_its_neighbors_place_and_the_counter_follows`
- `multi_rename::session_test::a_row_past_the_end_is_refused`
- `multi_rename::session_test::at_either_end_the_row_stays_and_the_preview_still_holds`
- `multi_rename::session_test::range_samples_refuse_a_token_that_isnt_a_range_and_wait_for_a_moved_rows_preview`
- `multi_rename::session_test::range_samples_say_what_a_range_takes_from_each_file_the_preview_lists`
- `redact::producer_tests::the_listing_lines_ship_their_dir_as_one_token_and_keep_the_rest`
- `tools::actions::multi_rename::tests::media::a_dropped_call_cancels_its_read`
- `tools::actions::multi_rename::tests::media::a_media_field_holding_a_private_key_reads_as_refused`
- `tools::actions::multi_rename::tests::media::a_media_mask_previews_exactly_as_the_sheet_does`
- `tools::actions::multi_rename::tests::media::a_media_row_on_a_protected_path_reads_nothing`
- `tools::actions::multi_rename::tests::media::an_open_sheets_answer_holding_a_private_key_is_not_taken`
- `tools::actions::multi_rename::tests::media::an_open_sheets_reads_are_reused_rather_than_read_again`
- `tools::actions::multi_rename::tests::media::answers_an_open_sheet_read_past_the_gate_are_not_taken`
- `tools::actions::multi_rename::tests::media::more_media_files_to_read_than_the_cap_is_a_typed_refusal`
- `tools::actions::multi_rename::tests::media::the_tools_media_read_sends_no_progress_event`
- `tools::actions::tests::test_user_path_param_expands_tilde` (cause unread)
- `tools::resources::importance::tests::tilde_in_a_path_expands_to_home` (cause unread)

### Not this plan: Windows behavior the product hasn't met yet: its Windows plan task (3)

- `instance_lock::tests::a_lock_held_on_any_one_of_the_three_dirs_refuses_a_second_claim`
- `instance_lock::tests::acquiring_stamps_our_pid_and_names_the_holder`
- `logging::rotating_file::tests::a_writer_rotated_away_by_another_reopens_the_live_file_instead_of_renaming_it`

### Not this plan: a Windows-only test expectation: #1232 and the Windows build state (9)

- `crash_reporter::next_launch_tests::crash_file_stamps_keep_their_bytes_and_read_back`
- `data_wipe::plan::tests::a_library_target_must_carry_the_identifier`
- `data_wipe::plan::tests::a_production_mac_plans_every_folder_and_file_cmdr_keeps`
- `data_wipe::plan::tests::an_e2e_run_nests_its_cache_and_logs_inside_the_data_dir`
- `data_wipe::plan::tests::linux_plans_no_library_state`
- `data_wipe::plan::tests::the_security_event_log_goes_with_the_wipe`
- `subprocess::tests::a_child_that_finishes_in_time_returns_its_output`
- `subprocess::tests::a_child_that_never_exits_is_killed_at_the_deadline`
- `tools::sensitive::platform::tests::only_absolute_values_count`

### Not this plan: a file another handle holds open: #1226 (6)

- `search::execute::tests::adb_phone::a_live_search_over_a_phone_nobody_indexed_walks_it` (cause unread)
- `search::execute::tests::goldens::search_answers_match_the_goldens` (cause unread)
- `search::execute::tests::goldens::the_other_platforms_goldens_hold_under_its_rules` (cause unread)
- `search::execute::tests::live_e2e::an_agent_search_walks_the_same_ground_and_gets_the_same_union` (cause unread)
- `search::execute::tests::source_equivalence::memory_holds_the_same_arena_the_index_loads`
- `search::index_source::tests::canonicalize_scope_path_expands_the_home_tilde` (cause unread)

### Not this plan: root-volume lookups: Windows plan task 34 (6)

- `commands::volumes::tests::resolve_location_sftp_path_returns_the_saved_server_without_dialing`
- `location::resolve::tests::a_home_relative_path_resolves_to_root`
- `tools::suggestions::tests::input::an_age_predicate_arrives_in_days_and_leaves_in_seconds`
- `tools::suggestions::tests::propose::a_selector_resolves_once_at_creation_and_freezes_what_it_found`
- `tools::suggestions::tests::propose::a_selector_that_matches_nothing_stages_nothing_and_says_so`
- `tools::suggestions::tests::propose::an_unreadable_index_refuses_differently_from_an_empty_one`

### S, `std::path` arithmetic on an app path: the app's file system and listings conversion (57)

- `about::history::tests::a_copy_off_another_volume_names_where_it_came_from`
- `about::history::tests::a_move_on_the_same_volume_names_no_volume`
- `about::history::tests::a_source_volume_that_is_not_connected_goes_unnamed`
- `about::history::tests::an_item_in_the_trash_says_where_it_was_trashed_from`
- `about::model::tests::a_path_the_gate_protects_leaves_name_level_facts_only` (cause unread)
- `about::tests::a_cancel_mid_way_stops_queued_jobs_and_leaves_no_late_events` (cause unread)
- `about::tests::a_container_is_never_looked_inside` (cause unread)
- `about::tests::a_file_with_no_download_record_says_so_as_a_routine_absence` (cause unread)
- `about::tests::a_modified_repo_file_answers_uncommitted_work_with_its_branch` (cause unread)
- `about::tests::a_row_past_its_budget_lands_as_timed_out_and_its_late_answer_drops` (cause unread)
- `about::tests::a_slow_card_cant_starve_the_next_cards_rows` (cause unread)
- `about::tests::library_folders_answer_their_owner_through_the_card` (cause unread)
- `about::through_tests::an_archive_entry_names_its_archive_with_the_archives_own_record` (cause unread)
- `about::verdict::tests::a_pictures_tags_and_text_ride_the_take_inside_the_fence` (cause unread)
- `about::verdict::tests::a_reopen_on_the_same_node_reuses_the_verdict` (cause unread)
- `about::verdict::tests::a_verdict_streams_after_the_plan_with_its_given_line_first` (cause unread)
- `about::verdict::tests::holding_contents_back_keeps_the_look_and_the_pictures_text_home` (cause unread)
- `commands::agent::suggested_ops_live::tests::only_a_renamed_op_has_a_landing_path`
- `commands::file_system::volume_copy::destination_root_echo_test::a_server_path_typed_on_a_server_place_names_both_readings`
- `commands::file_system::write_ops::tests::reject_if_routed_over_a_parent_flags_a_path_inside_a_zip` (cause unread)
- `commands::file_system::write_ops::tests::scan_preview_routes_an_archive_source_to_the_archive_volume` (cause unread)
- `commands::file_viewer::tests::write_range_rejects_a_destination_inside_an_archive` (cause unread)
- `commands::servers::server_moves_test::moving_an_sftp_server_takes_its_entry_password_and_favorites_to_the_new_address`
- `commands::servers::servers_test::saving_a_start_folder_under_the_root_stores_it_normalized`
- `favorites::target::tests::a_local_folder_is_stat_and_reads_yes_or_no` (cause unread)
- `favorites::target::tests::a_protected_folder_is_taken_on_trust_without_a_stat` (cause unread)
- `file_system::restic::overlay_tests::the_row_points_at_the_view_and_confirms_the_repo`
- `file_system::volume::manager::archive_routing::tests::mount_id_for_path_skips_a_registered_archive_and_names_its_parent_mount`
  (cause unread)
- `file_system::volume::manager::archive_routing::tests::path_is_inside_archive_confirms_a_local_inner_path_but_not_the_zip_itself`
  (cause unread)
- `file_system::volume::manager::archive_routing::tests::path_is_inside_archive_confirms_a_remote_inner_path_through_the_parent`
  (cause unread)
- `file_system::volume::manager::archive_routing::tests::resolve_evicts_the_least_recently_used_archive_past_the_cap`
  (cause unread)
- `file_system::volume::manager::archive_routing::tests::resolve_routes_a_remote_zip_via_the_parents_ranged_reads`
  (cause unread)
- `file_system::volume::manager::archive_routing::tests::resolve_routes_an_archive_inner_path_to_an_archive_volume`
  (cause unread)
- `go_to_path::tests::deep_nonexistent_path_falls_back_to_nearest_ancestor` (cause unread)
- `go_to_path::tests::dot_and_dotdot_are_lexically_normalized` (cause unread)
- `go_to_path::tests::dotdot_above_the_root_answers_the_root` (cause unread)
- `go_to_path::tests::dotdot_past_a_nonexistent_middle_segment_normalizes_lexically` (cause unread)
- `go_to_path::tests::existing_dir_resolves_to_directory` (cause unread)
- `go_to_path::tests::existing_file_resolves_to_file_with_parent_and_name` (cause unread)
- `go_to_path::tests::relative_input_joins_against_base_dir` (cause unread)
- `go_to_path::tests::tilde_expands_to_home` (cause unread)
- `go_to_path::tests::totally_nonexistent_absolute_path_falls_back_to_root`
- `go_to_path::tests::typing_a_path_into_a_zip_enters_it_as_a_directory` (cause unread)
- `listing_host::tests::archive_watch_integration_test::lru_eviction_releases_the_archive_and_its_watch` (cause unread)
- `listing_host::tests::foreign_path_test::a_listing_read_while_the_backend_is_replaced_lands_on_the_stored_spelling`
- `network::live_server_edit::live_server_edit_test::a_start_folder_alone_moves_the_landing_and_keeps_the_instance`
- `network::live_server_edit::live_server_edit_test::a_wider_root_the_server_has_is_installed_and_announced`
- `network::live_server_edit::live_server_edit_test::clearing_the_start_folder_lands_on_the_root`
- `network::nfs_known_places::nfs_known_places_test::an_export_spelled_two_ways_is_one_entry`
- `network::nfs_known_places::nfs_known_places_test::an_unnamed_server_reads_as_its_host_and_an_export_as_its_path`
  (cause unread)
- `network::nfs_known_places::nfs_known_places_test::resolved_params_save_the_split_and_the_settings`
- `network::nfs_volume_wiring::nfs_volume_wiring_test::a_changed_start_folder_on_an_open_place_moves_the_live_volume_params`
- `network::nfs_volume_wiring::nfs_volume_wiring_test::a_connect_registers_and_saves_under_the_resolved_export`
- `network::nfs_volume_wiring::nfs_volume_wiring_test::an_edit_to_a_place_nobody_has_open_saves_without_dialing`
- `network::saved_server_fields::saved_server_fields_test::a_connect_drops_a_saved_start_folder_the_root_no_longer_holds`
- `network::saved_server_fields::saved_server_fields_test::a_relative_start_folder_is_read_from_the_server_root`
- `network::saved_server_fields::saved_server_fields_test::a_start_folder_under_the_root_is_stored_normalized`

### S, `std::path` arithmetic on an app path: the app's operations and tools conversion (30)

- `adb::eject_test::ejecting_a_phone_retires_its_volume_keeps_its_row_and_the_next_dial_opens_a_new_one` (cause unread)
- `adb::volume_wiring::volume_wiring_test::a_pane_on_an_adb_path_lists_the_phone_through_the_listing_pipeline`
- `adb::volume_wiring::volume_wiring_test::the_viewer_opens_a_text_file_on_a_dialed_phone`
- `agent::line::tests::preview_tests::a_listing_shows_its_rows_with_paths_joined_onto_the_folder` (cause unread)
- `agent::line::tests::preview_tests::a_search_reports_its_match_count_and_lower_bound` (cause unread)
- `agent::line::tests::preview_tests::inspect_shows_a_row_per_find_hit_and_each_paths_status` (cause unread)
- `agent::line::tests::preview_tests::the_other_lists_show_their_rows` (cause unread)
- `agent::line::tests::subject_tests::a_memory_path_is_a_place_only_when_it_lands_inside_the_folder` (cause unread)
- `agent::line::tests::subject_tests::only_an_absolute_whole_path_is_a_place_to_go` (cause unread)
- `agent::line::tests::subject_tests::search_names_its_pattern_and_only_the_scopes_include_paths` (cause unread)
- `menu::menu_items::menu_items_test::the_undo_trash_label_names_what_it_would_put_back` (cause unread)
- `priority::roots::tests::a_tab_on_another_volume_is_ignored` (cause unread)
- `priority::roots::tests::only_favorites_on_an_os_path_become_priority_roots` (cause unread)
- `priority::roots::tests::the_focused_pane_s_active_tab_leads_the_tab_order` (cause unread)
- `priority::roots::tests::the_pre_tabs_keys_still_answer` (cause unread)
- `saved_servers::s3_accounts::s3_accounts_test::an_accounts_buckets_list_as_places_under_one_row` (cause unread)
- `saved_servers::saves::saves_test::a_place_edit_keeps_the_other_groups_and_adding_them_keeps_the_id`
- `server_volumes::server_volumes_test::a_live_servers_row_comes_out_of_the_pipeline_enriched` (cause unread)
- `server_volumes::server_volumes_test::a_pinned_saved_server_answers_for_its_own_paths_without_connecting` (cause
  unread)
- `server_volumes::server_volumes_test::a_pinned_webdav_server_gets_a_row_of_its_own_kind`
- `server_volumes::server_volumes_test::a_saved_servers_row_carries_its_start_folder_as_the_landing`
- `server_volumes::server_volumes_test::a_saved_webdav_server_answers_for_its_own_paths` (cause unread)
- `server_volumes::server_volumes_test::an_s3_path_in_a_saved_bucket_names_the_bucket_place_over_the_account_root`
  (cause unread)
- `server_volumes::server_volumes_test::an_unpinned_saved_server_still_answers_for_its_paths` (cause unread)
- `server_volumes::server_volumes_test::every_saved_server_gets_a_row_carrying_its_own_pin`
- `tools::actions::multi_rename::tests::a_proposal_stages_the_ready_rows_as_one_pending_rename_group`
- `tools::propose::rename::tests::plan::a_folder_on_a_dead_address_of_a_live_share_moves_onto_the_live_one`
- `tools::propose::rename::tests::plan::a_rebased_folder_rewrites_every_row_onto_the_volumes_current_root`
- `tools::propose::rename::tests::preflight::cycle_warnings_mark_only_closed_dependency_components` (cause unread)
- `tools::suggestions::tests::names::a_file_never_lands_on_another_source_of_its_group`

### Timed out: the app's file system and listings conversion (1)

- `about::through_tests::a_share_that_stopped_answering_gets_its_card_at_once_and_says_so_once`

### Timed out: the app's operations and tools conversion (3)

- `agent::wake::tests::coalesce::five_million_changes_in_one_folder_coalesce_to_one_exact_bundle`
- `search::execute::tests::adb_phone::a_search_scoped_to_an_indexed_phone_finds_its_files_as_phone_paths`
- `search::execute::tests::live_e2e::an_agent_that_stops_waiting_does_not_stop_the_walk`

## cmdr-adb (52)

### Not this plan: a Linux errno read through the host's codes: the Windows build state's backend errno sentence (1)

- `errors::errors_test::an_unlisted_device_errno_arrives_as_its_linux_class`

### S, `std::path` arithmetic on an app path: this crate's app path conversion (51)

- `volume::conformance_test::a_batch_scan_asks_its_boundary_inside_the_walk`
- `volume::conformance_test::a_batch_scan_stops_when_it_is_told_to`
- `volume::conformance_test::a_bounded_range_read_seeks_on_device_and_returns_only_the_requested_window`
- `volume::conformance_test::a_copy_keeps_the_source_date` (cause unread)
- `volume::conformance_test::a_copy_keeps_the_source_date_over_the_v1_verbs` (cause unread)
- `volume::conformance_test::a_copy_sends_the_sources_mode_and_a_modeless_one_lands_as_adb_push_would` (cause unread)
- `volume::conformance_test::a_create_new_write_lands_through_a_staging_sibling_and_leaves_no_partial` (cause unread)
- `volume::conformance_test::a_forced_rename_keeps_a_non_empty_folder` (cause unread)
- `volume::conformance_test::a_forced_rename_replaces_an_empty_folder_under_its_name`
- `volume::conformance_test::a_forceless_rename_refuses_an_existing_destination`
- `volume::conformance_test::a_link_to_a_folder_moves_as_the_link`
- `volume::conformance_test::a_read_over_stat_v2_declares_its_exact_length`
- `volume::conformance_test::a_read_over_the_v1_verbs_declares_no_exact_length`
- `volume::conformance_test::a_rename_of_a_missing_source_names_it_and_clears_nothing`
- `volume::conformance_test::a_replacing_write_streams_straight_onto_the_callers_name` (cause unread)
- `volume::conformance_test::a_replacing_write_whose_source_fails_removes_what_it_sent` (cause unread)
- `volume::conformance_test::an_unknown_length_create_new_refuses_and_preserves_an_existing_file` (cause unread)
- `volume::conformance_test::an_unknown_length_multichunk_write_streams_and_reports_its_actual_count` (cause unread)
- `volume::conformance_test::cancelling_a_read_mid_file_releases_the_socket_and_the_volume_keeps_working`
- `volume::conformance_test::copy_within_matches_what_is_offered` (cause unread)
- `volume::conformance_test::create_directory_all_goes_through_a_link_to_a_folder`
- `volume::conformance_test::create_directory_all_refuses_a_file_in_the_way`
- `volume::conformance_test::create_directory_all_reports_an_existing_directory_honestly` (cause unread)
- `volume::conformance_test::create_file_refuses_to_clobber`
- `volume::conformance_test::delete_leaves_a_non_empty_directory_intact`
- `volume::conformance_test::delete_of_a_link_to_a_folder_removes_the_link_alone`
- `volume::conformance_test::export_matches_the_bytes_offered`
- `volume::conformance_test::listing_dates_honor_the_shared_claim_contract`
- `volume::conformance_test::mkdir_on_an_existing_directory_succeeds`
- `volume::conformance_test::the_shared_metadata_contract` (cause unread)
- `volume::conformance_test::writability_matches_the_mutations_offered`
- `volume::conformance_test::write_from_stream_create_new_refuses_to_clobber`
- `volume::paths::paths_test::a_bare_device_path_and_another_devices_path_are_refused` (cause unread)
- `volume::paths::paths_test::a_dialog_path_anchored_at_the_root_lands_on_the_device_path`
- `volume::paths::paths_test::a_prefixed_and_a_relative_path_land_on_the_same_device_path`
- `volume::paths::paths_test::dot_dot_is_resolved_lexically`
- `volume::paths::paths_test::every_spelling_of_the_root_is_the_root` (cause unread)
- `volume::volume_impl_test::a_copy_scan_refuses_a_directory_the_phone_wont_open`
- `volume::volume_impl_test::a_link_to_a_folder_is_a_link_and_deletes_as_one`
- `volume::volume_impl_test::a_listing_maps_kinds_sizes_and_symlinked_folders`
- `volume::volume_impl_test::a_listing_of_a_directory_the_phone_wont_open_is_refused_not_empty`
- `volume::volume_impl_test::a_listing_of_a_missing_directory_carries_the_path`
- `volume::volume_impl_test::a_listing_through_the_app_prefix_reaches_the_device_and_hands_back_prefixed_paths` (cause
  unread)
- `volume::volume_impl_test::an_empty_folder_the_phone_can_read_lists_as_empty`
- `volume::volume_impl_test::an_index_walk_of_a_phone_descends_only_its_storage` (cause unread)
- `volume::volume_impl_test::an_index_walk_pays_no_shell_round_trip_for_an_empty_folder`
- `volume::volume_impl_test::connecting_names_the_device_and_says_hello` (cause unread)
- `volume::volume_impl_test::every_write_patches_the_pane_once` (cause unread)
- `volume::volume_impl_test::space_at_a_folder_that_does_not_exist_yet_is_the_filesystem_it_would_land_on` (cause
  unread)
- `volume::volume_impl_test::space_at_a_path_is_the_filesystem_holding_it` (cause unread)
- `volume::volume_impl_test::write_access_is_what_test_w_says_about_the_nearest_folder` (cause unread)

## cmdr-archive (11)

### S, `std::path` arithmetic on an app path: this crate's app path conversion (11)

- `boundary::tests::confirm_accepts_a_real_zip_file` (cause unread)
- `boundary::tests::confirm_matches_per_format_magic` (cause unread)
- `boundary::tests::inside_archive_distinguishes_the_zip_file_itself_from_its_contents` (cause unread)
- `boundary::tests::targets_archive_file_true_for_a_real_file_named_like_an_archive` (cause unread)
- `boundary_on_parent::boundary_on_parent_test::a_local_parent_confirms_a_real_zip_and_its_inner_paths` (cause unread)
- `boundary_on_parent::boundary_on_parent_test::a_remote_confirm_checks_the_named_formats_magic` (cause unread)
- `boundary_on_parent::boundary_on_parent_test::a_remote_parent_confirms_a_zip_through_its_own_reads` (cause unread)
- `boundary_on_parent::boundary_on_parent_test::a_remote_parent_without_ranged_reads_still_routes` (cause unread)
- `volume::volume_test::a_unix_mode_in_the_external_attributes_reaches_the_entry` (cause unread)
- `volume::volume_test::an_entry_with_no_recorded_mode_reports_the_no_concept_sentinel` (cause unread)
- `volume::volume_test::entry_paths_keep_a_url_rooted_archive_path_spelled_exactly`

## cmdr-index (157)

### N, a native tempdir where an app path belongs: this crate's app path conversion (41)

- `indexing::reconcile::local_reconcile::tests::a_reconcile_cancelled_after_discovering_a_dir_leaves_no_exact_size_lies`
  (cause unread)
- `indexing::reconcile::local_reconcile::tests::cancelled_reconcile_leaves_prior_index_and_writes_no_marks` (cause
  unread)
- `indexing::reconcile::local_reconcile::tests::cost_budget_walk::a_budget_skipped_subtree_keeps_every_row_and_its_sizes`
  (cause unread)
- `indexing::reconcile::local_reconcile::tests::cost_budget_walk::a_budget_skipped_subtree_leaves_its_epoch_and_every_ancestor_epoch_untouched`
  (cause unread)
- `indexing::reconcile::local_reconcile::tests::cost_budget_walk::a_subtree_over_the_read_budget_stops_being_descended`
  (cause unread)
- `indexing::reconcile::local_reconcile::tests::cost_budget_walk::a_subtree_within_the_read_budget_is_fully_walked`
  (cause unread)
- `indexing::reconcile::local_reconcile::tests::reconcile_after_real_fresh_scan_of_unchanged_tree_is_a_no_op` (cause
  unread)
- `indexing::reconcile::local_reconcile::tests::reconcile_descends_into_existing_unchanged_child_dirs` (cause unread)
- `indexing::reconcile::local_reconcile::tests::reconcile_empty_root_keeps_prior_index_and_signals_empty_root` (cause
  unread)
- `indexing::reconcile::local_reconcile::tests::reconcile_from_empty_index_builds_correct_aggregates` (cause unread)
- `indexing::reconcile::local_reconcile::tests::reconcile_removes_entry_deleted_on_disk` (cause unread)
- `indexing::reconcile::reconcile_correctness::tests::deleted_dir_unreached_by_interrupted_reconcile_heals_on_next_complete`
  (cause unread)
- `indexing::reconcile::reconcile_correctness::tests::partial_reconcile_keeps_a_fresh_b_stale`
- `indexing::reconcile::reconcile_correctness::tests::reconcile_handles_add_remove_modify_typechange` (cause unread)
- `indexing::reconcile::reconcile_correctness::tests::repeated_interrupted_then_complete_reconcile_leaves_no_orphans_or_ghosts`
  (cause unread)
- `indexing::reconcile::reconciler::rescan::hold::tests::an_established_anchor_holds_from_the_moment_it_is_queued`
  (cause unread)
- `indexing::reconcile::reconciler::tests::delete_gates::a_removal_event_on_a_listed_drive_still_deletes` (cause unread)
- `indexing::reconcile::reconciler::tests::directory_read::the_reconcile_read_matches_a_per_entry_stat` (cause unread)
- `indexing::reconcile::reconciler::tests::hardlinks::reconcile_deduped_hardlink_with_a_new_mtime_is_written` (cause
  unread)
- `indexing::reconcile::reconciler::tests::hardlinks::reconcile_deduped_hardlink_writes_nothing_on_a_repeat_pass` (cause
  unread)
- `indexing::reconcile::reconciler::tests::hardlinks::reconcile_sized_hardlink_still_compares_on_size` (cause unread)
- `indexing::reconcile::reconciler::tests::live_events::a_change_reports_only_the_dir_whose_listing_changed` (cause
  unread)
- `indexing::reconcile::reconciler::tests::live_events::a_folder_created_live_gets_walked_and_reads_exact` (cause
  unread)
- `indexing::reconcile::reconciler::tests::live_events::live_create_under_mount_rooted_index_resolves_via_strip` (cause
  unread)
- `indexing::reconcile::reconciler::tests::live_events::live_delete_under_mount_rooted_index_resolves_via_strip` (cause
  unread)
- `indexing::reconcile::reconciler::tests::live_events::live_throttle_collapses_rapid_rewrites_and_trailing_flushes_last_size`
  (cause unread)
- `indexing::reconcile::reconciler::tests::live_events::live_throttle_significant_jump_applies_immediately` (cause
  unread)
- `indexing::reconcile::reconciler::tests::live_events::process_dir_creation_writes_entry_and_propagates` (cause unread)
- `indexing::reconcile::reconciler::tests::live_events::process_file_creation_writes_entry` (cause unread)
- `indexing::reconcile::reconciler::tests::live_events::removal_event_for_existing_directory_upserts_not_deletes` (cause
  unread)
- `indexing::reconcile::reconciler::tests::must_scan_routing::a_deep_must_scan_buffered_during_a_scan_walks_on_the_drain`
  (cause unread)
- `indexing::reconcile::reconciler::tests::must_scan_routing::missing_parent_creation_escalates_to_highest_missing_dir`
  (cause unread)
- `indexing::reconcile::reconciler::tests::must_scan_routing::reconcile_subtree_missing_chain_escalates` (cause unread)
- `indexing::reconcile::reconciler::tests::replay::replay_calls_callback_with_affected_paths` (cause unread)
- `indexing::reconcile::reconciler::tests::replay::replay_processes_events_after_scan_start` (cause unread)
- `indexing::reconcile::reconciler::tests::subtree::reconcile_subtree_deep_nested_dirs` (cause unread)
- `indexing::reconcile::reconciler::tests::subtree::reconcile_subtree_indexes_new_directory_not_in_db` (cause unread)
- `indexing::reconcile::reconciler::tests::subtree::reconcile_subtree_marks_listed_dirs_at_current_epoch`
- `indexing::reconcile::reconciler::tests::subtree::reconcile_subtree_new_nested_dir_with_child` (cause unread)
- `indexing::reconcile::verifier::tests::verify_corrects_a_mount_rooted_volumes_own_index` (cause unread)
- `indexing::reconcile::verifier::tests::verify_scans_a_new_directory_into_a_mount_rooted_volumes_index` (cause unread)

### Not this plan: a Windows-only test expectation: #1232 and the Windows build state (3)

- `indexing::store::tests::path_resolution::normalize_for_comparison_is_identity_off_macos`
- `indexing::store::tests::path_resolution::platform_case_compare_is_binary_off_macos`
- `indexing::store::tests::path_resolution::platform_case_proptests::matches_byte_cmp_off_macos`

### Not this plan: a file another handle holds open: #1226 (47)

- `importance::store::legacy_split::tests::a_background_split_is_waited_for`
- `importance::store::legacy_split::tests::a_clock_set_back_after_a_split_still_adds_a_downgrades_visits`
- `importance::store::legacy_split::tests::a_crash_after_the_copy_only_deletes_the_old_store`
- `importance::store::legacy_split::tests::a_crash_after_the_token_redoes_the_copy_once`
- `importance::store::legacy_split::tests::a_crash_inside_the_copy_leaves_nothing_half_copied`
- `importance::store::legacy_split::tests::a_crash_inside_the_delete_leaves_sidecars_the_next_run_clears`
- `importance::store::legacy_split::tests::a_downgrade_round_trip_merges_both_histories`
- `importance::store::legacy_split::tests::a_failure_that_may_pass_leaves_the_old_store_whole`
- `importance::store::legacy_split::tests::a_restored_copy_of_a_split_store_adds_nothing`
- `importance::store::legacy_split::tests::a_second_run_after_a_full_split_does_nothing`
- `importance::store::legacy_split::tests::after_a_split_the_volume_is_due_its_full_pass`
- `importance::store::legacy_split::tests::an_empty_visit_table_still_splits_and_the_old_store_goes`
- `importance::store::legacy_split::tests::an_older_build_writing_under_a_marked_token_adds_only_what_it_wrote`
- `importance::store::legacy_split::tests::each_volume_splits_on_its_own`
- `importance::store::legacy_split::tests::the_visits_move_to_their_own_file_and_the_old_store_goes`
- `importance::store::legacy_split::tests::together_the_combined_store_splits_too`
- `importance::store::legacy_split::tests::visits_only_in_the_old_write_ahead_log_come_over_too`
- `indexing::handle::tests::building_splits_an_older_builds_importance_store`
- `indexing::lifecycle::cover::cold_drive_tests::moves::a_first_index_finishing_after_a_rename_leaves_no_branch_at_the_old_mount_point`
  (cause unread)
- `indexing::lifecycle::cover::repair_tests::a_repair_reports_the_directories_it_read` (cause unread)
- `indexing::lifecycle::cover::repair_tests::a_repair_whose_consumer_left_still_covers_the_ground` (cause unread)
- `indexing::lifecycle::cover::repair_tests::a_repaired_frontier_node_reports_the_rows_it_wrote` (cause unread)
- `indexing::lifecycle::cover::tests::a_chain_running_through_a_file_row_is_declined` (cause unread)
- `indexing::lifecycle::cover::tests::a_frontier_path_with_no_row_is_materialized_and_walked` (cause unread)
- `indexing::lifecycle::cover::tests::a_walk_emits_what_it_writes` (cause unread)
- `indexing::lifecycle::partial_agg::tests::keeps_matching_volume_absolute_paths_normalized_and_deduped` (cause unread)
- `indexing::lifecycle::phases::tests::completion::a_permanently_timing_out_directory_still_lets_completion_happen`
  (cause unread)
- `indexing::lifecycle::phases::tests::completion::a_subtree_pruned_by_the_failure_budget_still_lets_completion_happen`
  (cause unread)
- `indexing::lifecycle::phases::tests::coverage::a_changed_exclusion_fingerprint_rebuilds_a_phased_index` (cause unread)
- `indexing::lifecycle::phases::tests::coverage::a_completed_index_under_an_older_policy_is_rebuilt_and_re_stamped`
  (cause unread)
- `indexing::lifecycle::phases::tests::coverage::a_fresh_phased_volume_s_frontier_shrinks_after_one_walk` (cause unread)
- `indexing::lifecycle::phases::tests::coverage::frontier_excludes_covered_ground_after_a_stitch` (cause unread)
- `indexing::lifecycle::phases::tests::coverage::start_scan_refuses_while_a_phase_is_active` (cause unread)
- `indexing::lifecycle::phases::tests::interleaving::a_folder_the_user_opens_stops_the_walk_of_a_big_sibling` (cause
  unread)
- `indexing::lifecycle::phases::tests::interleaving::a_visited_root_is_taken_between_frontier_roots` (cause unread)
- `indexing::lifecycle::phases::tests::interleaving::tiny_frontier_roots_are_walked_several_to_a_call` (cause unread)
- `indexing::lifecycle::phases::tests::menu_actions::forgetting_a_half_covered_drive_takes_the_branch_set_with_the_database`
  (cause unread)
- `indexing::lifecycle::phases::tests::menu_actions::stopping_a_half_covered_drive_leaves_it_resumable` (cause unread)
- `indexing::lifecycle::phases::tests::relaunch::a_legacy_interrupted_partial_is_thrown_away_and_rebuilt` (cause unread)
- `indexing::lifecycle::phases::tests::relaunch::a_master_switch_cycle_resumes_the_phases_instead_of_rebuilding` (cause
  unread)
- `indexing::lifecycle::phases::tests::relaunch::a_stopped_phased_index_comes_back_with_its_rows` (cause unread)
- `indexing::lifecycle::phases::tests::relaunch::an_automatic_rescan_restarts_the_phases_instead_of_truncating` (cause
  unread)
- `indexing::lifecycle::phases::tests::relaunch::the_kill_switch_gives_a_phased_partial_back_to_the_bulk_scan` (cause
  unread)
- `indexing::lifecycle::phases::tests::retry::a_machine_that_stops_short_asks_for_another_go` (cause unread)
- `indexing::lifecycle::phases::tests::retry::the_retry_completes_the_drive_once_the_churn_stops` (cause unread)
- `indexing::tests::integration_tests::read_pool_over_a_recreated_database_never_serves_the_old_one`
- `indexing::tests::stress_tests_concurrency::mixed_storm_reaches_consistent_fixed_point` (cause unread)

### S, `std::path` arithmetic on an app path: this crate's app path conversion (17)

- `indexing::scanner::convergence_tests::a_cancelled_walk_leaves_durable_partial_coverage`
- `indexing::scanner::convergence_tests::a_folder_the_walk_cannot_read_stops_re_entering_the_frontier` (cause unread)
- `indexing::scanner::convergence_tests::a_frontier_node_can_hold_a_listed_descendant` (cause unread)
- `indexing::scanner::convergence_tests::a_wedged_dir_on_a_drive_that_stays_is_still_condemned` (cause unread)
- `indexing::scanner::convergence_tests::covering_a_frontier_node_never_removes_a_row_it_did_not_write` (cause unread)
- `indexing::scanner::convergence_tests::marking_abandoned_ground_costs_no_coverage` (cause unread)
- `indexing::scanner::policy_tests::a_boot_disk_walk_stops_at_a_mount_inside_the_boot_tree` (cause unread)
- `indexing::scanner::policy_tests::a_search_walk_cuts_where_another_volume_is_mounted` (cause unread)
- `indexing::scanner::policy_tests::a_search_walk_skips_a_structurally_excluded_child` (cause unread)
- `indexing::scanner::policy_tests::a_search_walk_stops_at_a_mount_inside_the_boot_tree` (cause unread)
- `indexing::watch::branches::tests::an_event_inside_a_covered_branch_reaches_the_index_and_one_outside_does_not` (cause
  unread)
- `indexing::watch::branches::tests::an_event_that_lands_mid_walk_is_not_lost` (cause unread)
- `indexing::watch::branches::tests::the_branch_collapse_is_visible_to_the_running_live_loop`
- `indexing::watch::branches::tests::the_branch_set_survives_a_restart_through_the_volumes_own_database`
- `indexing::watch::event_loop::tests::replay_chunks::a_deferred_rescan_commits_with_the_position_that_passes_its_event`
  (cause unread)
- `indexing::watch::event_loop::tests::replay_chunks::a_drained_window_commits_its_rows_with_its_position_and_nothing_past_it`
  (cause unread)
- `indexing::watch::event_loop::tests::replay_chunks::a_replay_that_resumes_from_the_last_chunk_applies_every_change_once`
  (cause unread)

### Timed out: this crate's app path conversion (49)

- `indexing::lifecycle::cover::cold_drive_tests::activation::a_cold_volume_bootstraps_and_claims_only_what_it_walked`
- `indexing::lifecycle::cover::cold_drive_tests::activation::a_second_walk_reuses_the_index_the_first_one_stood_up`
- `indexing::lifecycle::cover::cold_drive_tests::activation::an_index_that_predates_the_exclusion_policy_is_dropped_before_the_next_walk`
- `indexing::lifecycle::cover::cold_drive_tests::activation::turning_indexing_on_after_a_walk_covers_the_drive_without_truncating_it`
- `indexing::lifecycle::cover::cold_drive_tests::branches::a_branch_comes_back_when_the_volume_does`
- `indexing::lifecycle::cover::cold_drive_tests::branches::a_change_inside_a_walked_branch_reaches_the_index_and_one_beside_it_does_not`
- `indexing::lifecycle::cover::cold_drive_tests::branches::a_vetoed_drive_is_walked_and_left_unwatched`
- `indexing::lifecycle::cover::cold_drive_tests::branches::a_walk_leaves_the_ground_it_covered_watched_and_written_down`
- `indexing::lifecycle::cover::cold_drive_tests::branches::clearing_a_drives_index_drops_the_branches_with_the_coverage`
- `indexing::lifecycle::cover::cold_drive_tests::intent::a_searched_drive_is_never_recorded_as_one_the_user_turned_on`
- `indexing::lifecycle::cover::cold_drive_tests::intent::enabling_a_drive_a_search_walked_records_the_choice_too`
- `indexing::lifecycle::cover::cold_drive_tests::moves::a_drive_renamed_while_it_indexes_keeps_indexing_at_its_new_mount_point`
- `indexing::lifecycle::cover::cold_drive_tests::moves::a_drive_turned_off_before_its_rename_stays_off`
- `indexing::lifecycle::cover::cold_drive_tests::moves::a_rename_between_a_walks_last_row_and_its_release_keeps_the_index`
- `indexing::lifecycle::cover::cold_drive_tests::moves::a_rename_inside_a_drain_window_moves_the_start_that_window_carries`
- `indexing::lifecycle::cover::cold_drive_tests::moves::a_rename_while_a_scan_starts_restarts_the_drive_at_the_handback`
- `indexing::lifecycle::cover::cold_drive_tests::removals::a_stop_that_lands_during_a_scan_start_waits_for_the_handback`
- `indexing::lifecycle::cover::cold_drive_tests::removals::a_stop_that_meets_a_drain_in_flight_waits_for_it`
- `indexing::lifecycle::cover::cold_drive_tests::removals::a_stop_waits_for_a_search_walk_still_reading_the_drive_and_stops_it`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::a_clear_during_the_extraction_window_really_clears`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::a_disable_during_the_extraction_window_stops_the_drive_and_records_the_veto`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::a_drive_that_stopped_indexing_is_owed_no_rescan`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::a_remembered_rescan_waits_for_the_last_walk_out`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::a_rescan_asked_for_during_a_scan_runs_when_that_scan_ends`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::a_rescan_during_the_phased_window_starts_the_machine_under_a_live_walk`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::a_rescan_refused_under_a_walk_runs_when_the_walk_ends`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::a_stop_during_the_extraction_window_really_stops`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::a_storage_failure_during_the_extraction_window_still_fails_the_volume`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::a_truncating_rescan_refuses_while_a_search_cover_walk_is_live`
- `indexing::lifecycle::cover::cold_drive_tests::rescans::clicking_rescan_five_times_during_a_scan_queues_one_walk`
- `indexing::lifecycle::cover::cold_drive_tests::space::a_drive_pauses_while_the_disk_is_low_and_resumes_when_there_is_room`
- `indexing::lifecycle::cover::cold_drive_tests::space::a_pause_landing_mid_scan_start_still_ends_paused`
- `indexing::lifecycle::cover::cold_drive_tests::space::a_paused_external_drive_keeps_serving_its_sizes_and_rescans_itself_when_room_returns`
- `indexing::lifecycle::cover::cold_drive_tests::space::a_watchdog_stop_during_the_pause_keeps_the_drive_stopped_when_space_returns`
- `indexing::lifecycle::cover::cold_drive_tests::space::stopping_a_paused_drive_keeps_it_stopped_when_space_returns`
- `indexing::lifecycle::cover::cold_drive_tests::switches::a_search_walks_a_drive_the_user_turned_indexing_off_for`
- `indexing::lifecycle::cover::cold_drive_tests::switches::a_search_walks_a_drive_with_the_master_switch_off`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::a_clear_claimed_on_a_detached_drive_waits_for_the_volume_to_be_let_go`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::a_clear_waits_for_the_volume_to_be_let_go_before_deleting_its_database`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::a_forget_during_a_stop_deletes_the_index_once_the_stop_is_done`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::a_start_on_a_detached_drive_lets_its_manager_come_back`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::a_start_that_meets_a_dead_index_rebuilds_it`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::a_stop_after_a_forget_in_one_drain_still_deletes_the_index`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::rapid_alternating_toggles_settle_on_the_last_one`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::turning_indexing_off_then_on_inside_the_drain_window_leaves_the_drive_indexing`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::turning_indexing_on_then_off_inside_the_drain_window_leaves_the_drive_off`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::two_teardowns_and_a_start_during_the_first_index_end_with_a_rebuilt_index`
- `indexing::lifecycle::cover::cold_drive_tests::toggles::two_teardowns_and_a_start_in_one_window_end_with_a_rebuilt_index`
- `indexing::reconcile::reconciler::tests::live_events::a_file_indexed_while_a_walk_holds_its_listing_stays_indexed`

## cmdr-listing (3)

### S, `std::path` arithmetic on an app path: this crate's app path conversion (3)

- `foreign_path::foreign_path_test::a_backend_swap_respells_an_open_listing_and_its_entries`
- `stall_test::a_read_that_never_answers_reports_the_listing_stalled_within_the_deadline` (cause unread)
- `stalled_on::tests::resolves_a_symlink_into_its_target` (cause unread)

## cmdr-listing-wasm (7)

### S, `std::path` arithmetic on an app path: this crate's app path conversion (6)

- `folder_sizes_test::a_long_delete_tells_a_pane_above_it_in_a_few_coalesced_events`
- `folder_sizes_test::a_write_tells_each_pane_above_it_its_moved_row_and_its_own_total`
- `imports_test::a_dropped_folder_lands_staged_with_its_structure_and_bytes`
- `search_test::a_scope_resolves_through_the_fold_and_a_stranger_comes_back_unresolved` (cause unread)
- `search_test::a_write_reaches_the_next_search`
- `viewer_test::markdown_renders_follows_a_relative_link_and_serves_its_image` (cause unread)

### Timed out: this crate's app path conversion (1)

- `folder_sizes_test::a_copy_and_a_delete_recount_every_pane_above_them`

## cmdr-mtp (2)

### S, `std::path` arithmetic on an app path: this crate's app path conversion (2)

- `connection::same_name_test::renaming_a_twin_row_renames_that_object_and_leaves_the_other`
- `volume::host_seam_test::a_walk_over_a_directory_reports_nothing_however_many_entries_it_holds` (cause unread)

## cmdr-nfs (49)

### Not this plan: a Linux errno read through the host's codes: the Windows build state's backend errno sentence (3)

- `errors::errors_test::a_lost_connection_is_a_transport_fault_and_a_refusal_is_not`
- `errors::errors_test::the_errno_shaped_statuses_carry_their_errno`
- `errors::errors_test::the_path_carrying_statuses_carry_the_path_and_never_server_text`

### S, `std::path` arithmetic on an app path: this crate's app path conversion (46)

- `params::params_test::a_place_with_no_port_is_minted_on_the_nfs_port`
- `volume::fake_volume_test::a_deep_folder_costs_no_lookup_once_its_parent_was_listed`
- `volume::fake_volume_test::a_dropped_connection_comes_back_unattended_with_nothing_to_offer`
- `volume::fake_volume_test::a_file_streams_byte_exact_through_reordered_and_short_answers`
- `volume::fake_volume_test::a_link_inside_the_export_is_followed_and_one_outside_it_is_only_a_link`
- `volume::fake_volume_test::a_listing_spans_pages_reports_each_batch_and_leaves_out_what_it_cant_name`
- `volume::fake_volume_test::a_missing_path_is_not_found_and_carries_the_path`
- `volume::fake_volume_test::a_mutation_finds_a_folder_another_client_renamed_away`
- `volume::fake_volume_test::a_server_without_readdirplus_lists_the_same_through_readdir_and_lookup`
- `volume::fake_volume_test::a_silent_server_is_disconnected_inside_the_ladder_and_every_waiting_call_answers` (cause
  unread)
- `volume::fake_volume_test::a_stale_cached_handle_is_looked_up_once_more_and_the_listing_lands`
- `volume::fake_volume_test::a_stream_declares_its_length_date_and_mode_before_the_first_byte`
- `volume::fake_volume_test::a_typed_path_under_the_export_resolves_to_the_export_and_a_start_folder`
- `volume::fake_volume_test::listing_dates_honor_the_shared_claim_contract`
- `volume::fake_volume_test::our_own_change_in_a_folder_keeps_its_children_vouched_for`
- `volume::fake_volume_test::read_range_answers_exactly_the_window_and_short_only_at_the_end`
- `volume::fake_volume_test::space_comes_from_fsstat_and_write_access_from_access`
- `volume::mutation::mutation_test::a_forced_rename_replaces_in_one_call`
- `volume::mutation::mutation_test::a_forceless_folder_rename_claims_with_mkdir`
- `volume::mutation::mutation_test::a_forceless_rename_claims_the_name_then_lands_onto_the_claim`
- `volume::mutation::mutation_test::a_forceless_rename_refuses_a_taken_name_and_touches_neither`
- `volume::mutation::mutation_test::a_mutation_acts_on_the_folder_the_path_names_now`
- `volume::mutation::mutation_test::apply_metadata_dates_and_modes_a_folder` (cause unread)
- `volume::mutation::mutation_test::create_directory_refuses_a_taken_name_and_mkdir_all_is_honest`
- `volume::mutation::mutation_test::delete_removes_a_file_an_empty_folder_and_a_link_but_never_recurses`
- `volume::poll::poll_test::a_change_another_client_made_is_seen_within_one_tick_and_reported_once`
- `volume::poll::poll_test::a_change_for_a_directory_no_pane_shows_any_more_is_dropped`
- `volume::poll::poll_test::a_tick_is_one_getattr_per_shown_directory_and_nothing_else`
- `volume::poll::poll_test::nothing_is_sent_while_the_volume_is_down`
- `volume::poll::poll_test::with_no_pane_open_nothing_is_sent_and_the_poller_stops`
- `volume::reconnect_test::a_read_cut_mid_file_resumes_on_the_next_generation_at_its_offset`
- `volume::reconnect_test::a_read_in_flight_finishes_on_its_own_connection_across_a_reconnect`
- `volume::reconnect_test::a_refused_redial_keeps_backing_off_and_comes_back_on_its_own`
- `volume::reconnect_test::a_start_folder_edit_moves_the_params_every_instance_and_the_next_redial_see` (cause unread)
- `volume::writes::writes_test::a_cancel_from_the_progress_callback_stops_and_removes_the_partial`
- `volume::writes::writes_test::a_short_write_is_resent_from_where_the_server_stopped`
- `volume::writes::writes_test::a_source_shorter_than_it_declared_fails_and_removes_the_partial`
- `volume::writes::writes_test::a_verifier_that_changes_among_the_writes_resends_the_span`
- `volume::writes::writes_test::a_verifier_that_changes_before_the_commit_resends_the_span_and_lands`
- `volume::writes::writes_test::a_verifier_that_changes_twice_fails_the_write_and_leaves_nothing`
- `volume::writes::writes_test::a_write_across_two_commit_spans_lands_every_byte_and_commits_each`
- `volume::writes::writes_test::create_file_lands_its_content_and_refuses_to_clobber`
- `volume::writes::writes_test::create_new_refuses_a_taken_name_and_leaves_it_alone`
- `volume::writes::writes_test::create_or_replace_overwrites_a_longer_file_whole` (cause unread)
- `volume::writes::writes_test::no_write_carries_more_than_the_servers_wtmax`
- `volume::writes::writes_test::the_create_carries_only_the_mode_and_setattr_follows_the_last_write`

## cmdr-operation-log (3)

### S, `std::path` arithmetic on an app path: this crate's app path conversion (3)

- `query::landed::tests::the_node_an_operation_landed_names_that_operation`
- `query::tests::get_operation_returns_header_and_paged_items`
- `store::tests::intern_dir_round_trips_a_scheme_rooted_path`

## cmdr-operations (297)

### N, a native tempdir where an app path belongs: this crate's app path conversion (225)

- `approved_op_parity_tests::an_approved_copy_creates_a_missing_destination_folder_exactly_as_a_user_started_one_does`
  (cause unread)
- `approved_op_parity_tests::an_approved_copy_of_a_whole_folder_matches_the_user_started_one` (cause unread)
- `approved_op_parity_tests::an_approved_copy_overwrites_exactly_as_a_user_started_one_does` (cause unread)
- `approved_op_parity_tests::an_approved_copy_skips_exactly_where_a_user_started_one_skips` (cause unread)
- `approved_op_parity_tests::an_approved_move_takes_the_source_exactly_as_a_user_started_one_does` (cause unread)
- `approved_op_parity_tests::only_a_source_that_changed_is_treated_differently` (cause unread)
- `archive_edit::copy_into_remote_tests::copy_into_from_a_remote_source_lands_a_nested_tree`
- `archive_edit::copy_into_remote_tests::copy_into_from_a_remote_source_lands_the_file` (cause unread)
- `archive_edit::copy_into_remote_tests::copy_into_from_a_remote_source_uses_the_real_bytes_not_the_lying_size` (cause
  unread)
- `archive_edit::copy_into_remote_tests::move_into_from_a_remote_source_deletes_the_remote_originals_after_commit`
- `archive_edit::fresh_zip::tests::producer_preserves_names_empty_entries_metadata_and_level` (cause unread)
- `archive_edit::remote::remote_outage_tests::a_cancel_while_the_swap_waits_after_the_move_aside_keeps_the_original`
  (cause unread)
- `archive_edit::remote::remote_outage_tests::a_cancel_while_the_upload_waits_leaves_the_original` (cause unread)
- `archive_edit::remote::remote_outage_tests::a_drop_at_a_one_step_replace_waits_and_lands_without_uploading_again`
  (cause unread)
- `archive_edit::remote::remote_outage_tests::a_drop_at_the_pull_waits_and_pulls_again` (cause unread)
- `archive_edit::remote::remote_outage_tests::a_drop_at_the_upload_waits_and_uploads_again` (cause unread)
- `archive_edit::remote::remote_outage_tests::a_drop_inside_the_swap_waits_and_lands_without_uploading_again` (cause
  unread)
- `archive_edit::remote::remote_outage_tests::a_refused_upload_fails_at_once` (cause unread)
- `archive_edit::remote::remote_outage_tests::a_share_that_stays_gone_after_the_move_aside_keeps_the_original_for_its_return`
  (cause unread)
- `archive_edit::remote::remote_outage_tests::a_share_that_stays_gone_at_a_one_step_replace_leaves_the_original_archive`
  (cause unread)
- `archive_edit::remote::remote_outage_tests::a_share_that_stays_gone_at_the_upload_reports_the_lost_connection` (cause
  unread)
- `archive_edit::remote::remote_outage_tests::a_stale_error_on_the_swaps_move_aside_runs_the_swap_again` (cause unread)
- `archive_edit::remote::remote_tests::a_crash_after_any_step_of_the_swap_loses_no_archive` (cause unread)
- `archive_edit::remote::remote_tests::a_landing_that_fails_puts_the_original_back` (cause unread)
- `archive_edit::remote::remote_tests::a_name_that_refuses_both_the_edit_and_the_original_keeps_both_findable` (cause
  unread)
- `archive_edit::remote::remote_tests::a_refused_move_aside_leaves_the_original_whole_and_takes_the_temp_away` (cause
  unread)
- `archive_edit::remote::remote_tests::a_remote_edit_journals_as_a_zip_edit_never_as_an_overwrite` (cause unread)
- `archive_edit::remote::remote_tests::a_replacing_rename_that_landed_despite_its_error_keeps_the_edit` (cause unread)
- `archive_edit::remote::remote_tests::an_aside_that_wont_delete_stays_beside_the_landed_edit` (cause unread)
- `archive_edit::remote::remote_tests::remote_edit_adds_an_entry_and_swaps_it_into_place` (cause unread)
- `archive_edit::remote::remote_tests::remote_edit_ignores_a_stale_temp_for_a_different_archive` (cause unread)
- `archive_edit::remote::remote_tests::remote_edit_keeps_a_fresh_temp_for_the_same_archive` (cause unread)
- `archive_edit::remote::remote_tests::remote_edit_reap_delete_failure_does_not_fail_the_edit` (cause unread)
- `archive_edit::remote::remote_tests::remote_edit_reap_spares_a_temp_the_ledger_records` (cause unread)
- `archive_edit::remote::remote_tests::remote_edit_reaps_a_stale_temp_for_the_same_archive` (cause unread)
- `archive_edit::remote::remote_tests::remote_edit_swaps_via_delete_then_rename_on_a_sibling_allowing_backend` (cause
  unread)
- `archive_edit::remote::remote_tests::two_asides_claiming_one_empty_name_both_stay_put` (cause unread)
- `conflict::build_conflict_event_tests::type_flags_come_from_the_side_not_from_a_link_following_stat` (cause unread)
- `create::tests::create_directory_already_exists` (cause unread)
- `create::tests::create_directory_core_rejects_a_target_inside_an_archive` (cause unread)
- `create::tests::create_directory_managed_creates_folder_and_cleans_up_record` (cause unread)
- `create::tests::create_directory_managed_journals_a_create_folder_op`
- `create::tests::create_directory_success` (cause unread)
- `create::tests::create_file_already_exists` (cause unread)
- `create::tests::create_file_success` (cause unread)
- `create::tests::route_archive_create_on_an_existing_inner_name_errors_without_building_a_temp` (cause unread)
- `create::tests::synthetic_entry_diff_lands_only_in_listings_on_the_creates_volume` (cause unread)
- `create::tests::synthetic_entry_diff_refreshes_a_row_the_listing_already_holds`
- `free_space_tests::a_resync_onto_a_folder_that_holds_most_of_it_needs_only_what_changed` (cause unread)
- `golden_trace_tests::golden_local_conflict_skip` (cause unread)
- `golden_trace_tests::golden_local_copy` (cause unread)
- `golden_trace_tests::golden_local_delete` (cause unread)
- `golden_trace_tests::golden_local_instant`
- `golden_trace_tests::golden_local_move`
- `golden_trace_tests::golden_local_undo_copy` (cause unread)
- `home_folder::tests::a_rename_of_another_users_home_folder_goes_ahead`
- `in_flight_temps::claim_tests::an_engine_that_never_claims_leaves_another_runs_ledger_and_its_temps_alone` (cause
  unread)
- `in_flight_temps::tests::a_build_without_kinded_records_skips_them_rather_than_acting_on_them` (cause unread)
- `in_flight_temps::tests::a_deferred_record_is_swept_the_moment_its_volume_arrives` (cause unread)
- `in_flight_temps::tests::a_delete_the_volume_refuses_leaves_the_record_to_retry` (cause unread)
- `in_flight_temps::tests::a_landed_temp_is_retired_from_the_log` (cause unread)
- `in_flight_temps::tests::a_leftover_on_a_removable_drive_waits_for_that_drive_rather_than_being_forgotten` (cause
  unread)
- `in_flight_temps::tests::a_leftover_on_the_mac_stays_local_homed` (cause unread)
- `in_flight_temps::tests::a_local_record_stays_a_bare_path_on_disk` (cause unread)
- `in_flight_temps::tests::a_partial_with_no_named_path_space_is_kept_in_memory_but_not_persisted` (cause unread)
- `in_flight_temps::tests::a_record_whose_volume_isnt_reachable_survives_for_the_next_launch` (cause unread)
- `in_flight_temps::tests::a_recorded_orphan_is_swept_at_startup_however_fresh_it_is` (cause unread)
- `in_flight_temps::tests::a_recorded_partial_on_a_volume_is_swept_through_that_volume` (cause unread)
- `in_flight_temps::tests::a_volume_path_that_is_already_gone_is_counted_not_swallowed` (cause unread)
- `in_flight_temps::tests::an_aside_is_on_disk_before_track_returns_while_a_temp_only_queues` (cause unread)
- `in_flight_temps::tests::compaction_keeps_the_records_waiting_for_a_volume` (cause unread)
- `in_flight_temps::tests::compaction_shrinks_the_log_without_forgetting_what_is_still_in_flight` (cause unread)
- `in_flight_temps::tests::retiring_a_temp_clears_both_ledgers` (cause unread)
- `in_flight_temps::tests::the_sweep_refuses_a_volume_path_that_isnt_one_of_our_scratch_files` (cause unread)
- `journal_capture_tests::grouped_copy_journals_leaf_files_and_created_dir_rows`
- `journal_capture_tests::initiator_threads_from_the_op_into_the_journal_row`
- `journal_capture_tests::local_copy_header_carries_planned_count_items_done_and_dest_volume`
- `journal_capture_tests::overwriting_copy_finalizes_not_rollbackable`
- `journal_row_shape_tests::a_copy_journals_no_row_for_a_file_it_skipped_or_a_folder_it_found`
- `journal_row_shape_tests::a_folder_copy_journals_leaves_across_and_its_folders_in_place_on_the_dest_side`
- `journal_row_shape_tests::a_search_leaf_with_no_known_landing_is_recorded_source_only` (cause unread)
- `queued_sources::tests::a_queued_move_whose_sources_partly_left_asks_then_moves_the_rest` (cause unread)
- `rename::tests::a_batch_as_a_move_reports_the_swaps_it_left_out` (cause unread)
- `rename::tests::an_instant_rename_journals_its_row_through_the_real_store`
- `rename::tests::rename_managed_local_conflict_without_force_is_transparent` (cause unread)
- `rename::tests::rename_managed_local_success` (cause unread)
- `rename::tests::rename_managed_renames_a_zip_file_itself`
- `rename::tests::route_archive_rename_onto_an_existing_name_errors_without_building_a_temp` (cause unread)
- `reversal::tests::a_file_edited_in_place_is_left_alone` (cause unread)
- `reversal::tests::a_file_still_as_the_copy_wrote_it_is_removed` (cause unread)
- `reversal::tests::a_same_size_replacement_is_left_alone` (cause unread)
- `routing_table_tests::copy_on_root_runs_the_local_engine` (cause unread)
- `routing_table_tests::local_move_on_root_runs_the_local_engine` (cause unread)
- `routing_table_tests::mkdir_on_root_runs_the_local_engine`
- `routing_table_tests::mkfile_on_root_runs_the_local_engine`
- `routing_table_tests::move_on_root_runs_the_local_engine` (cause unread)
- `routing_table_tests::rename_on_root_runs_the_local_engine`
- `scan::sidecar_tests::a_copy_on_apfs_keeps_dot_underbar_files_with_and_without_a_sibling`
- `scan::tests::a_dry_run_of_a_copy_onto_a_different_file_still_reports_the_conflict` (cause unread)
- `scan::tests::online_only_tally::a_symlink_in_the_tree_is_never_asked_about` (cause unread)
- `scan::tests::walk_reports_a_symlinked_directory_source_as_a_file` (cause unread)
- `scan_preview_tests::a_transfer_scan_counts_the_names_windows_cant_use` (cause unread)
- `scan_preview_tree_tests::a_delete_or_compress_preview_builds_no_tree` (cause unread)
- `scan_preview_tree_tests::a_transfer_preview_of_a_local_selection_carries_its_tree_and_each_files_node` (cause unread)
- `scan_preview_tree_tests::an_operation_without_a_preview_tree_builds_one_from_the_walk` (cause unread)
- `settle_event_tests::a_completed_op_emits_complete_then_finalizes_then_frees_then_settles` (cause unread)
- `settle_event_tests::an_op_queued_behind_a_copy_reads_its_row_finalized_when_admitted` (cause unread)
- `settle_event_tests::injected_sink_receives_complete_and_settled_for_local_copy` (cause unread)
- `source_binding::tests::a_journaled_mtime_equals_the_live_reading_undo_rechecks_it_against`
- `source_binding::tests::a_source_replaced_by_a_different_file_of_the_same_size_is_dropped` (cause unread)
- `transfer::conflict_policy_latch_tests::a_one_off_answer_leaves_the_policy_on_ask_for_each`
- `transfer::conflict_policy_latch_tests::an_overwrite_all_answer_becomes_the_reported_policy`
- `transfer::conflict_prompt_sides_tests::a_copied_file_meeting_a_folder_is_described_as_a_file`
- `transfer::conflict_prompt_sides_tests::a_copied_folder_meeting_a_file_is_described_as_the_folder`
- `transfer::conflict_prompt_sides_tests::a_copied_link_to_a_folder_meeting_a_folder_is_described_as_a_leaf` (cause
  unread)
- `transfer::conflict_prompt_sides_tests::a_moved_file_meeting_a_folder_is_described_as_a_file` (cause unread)
- `transfer::conflict_prompt_sides_tests::a_moved_folder_meeting_a_file_is_described_as_the_folder` (cause unread)
- `transfer::conflict_prompt_sides_tests::a_moved_folder_meeting_a_link_to_a_folder_is_described_as_folder_over_leaf`
  (cause unread)
- `transfer::conflict_prompt_sides_tests::a_moved_link_to_a_folder_meeting_a_folder_is_described_as_a_leaf` (cause
  unread)
- `transfer::conflict_prompt_sides_tests::a_nested_copied_folder_meeting_a_file_is_described_as_that_folder`
- `transfer::conflict_prompt_sides_tests::a_partly_counted_folder_is_sized_as_a_lower_bound`
- `transfer::copy::copy_failure_tests::a_failed_folder_over_file_overwrite_keeps_the_users_file_beside_the_folder`
  (cause unread)
- `transfer::copy::copy_failure_tests::a_failed_folder_over_file_overwrite_names_the_recovered_path`
- `transfer::copy::copy_failure_tests::a_mid_batch_failure_keeps_the_file_that_replaced_the_users_original` (cause
  unread)
- `transfer::copy::copy_failure_tests::an_expected_refusal_raises_no_error_report`
- `transfer::copy::folder_dates_tests::a_copied_folder_keeps_its_date_and_so_do_the_folders_inside_it`
- `transfer::copy::folder_dates_tests::a_merge_leaves_the_existing_folders_date_alone`
- `transfer::copy::named_copy_tests::named_copy_honors_skip_and_overwrite_at_the_requested_name`
- `transfer::copy::named_copy_tests::named_copy_lands_beside_source_without_changing_it`
- `transfer::copy::named_copy_tests::named_copy_of_itself_duplicates_instead_of_overwriting`
- `transfer::copy::named_copy_tests::named_folder_copy_maps_children_and_empty_directories`
- `transfer::copy::refused_class_tests::a_stop_ends_the_copy_with_the_refusal` (cause unread)
- `transfer::copy::refused_class_tests::a_warned_name_still_lands`
- `transfer::copy::rollback::tests::an_untouched_copy_reverses_completely` (cause unread)
- `transfer::copy::rollback::tests::the_bar_reaches_zero_even_when_the_reversal_leaves_files_behind` (cause unread)
- `transfer::copy::rollback::tests::the_rollback_pace_holds_each_item_of_the_reversal` (cause unread)
- `transfer::copy_strategy::tests::a_chunked_many_small_file_copy_pays_one_barrier_at_the_close`
- `transfer::copy_strategy::tests::test_copy_file_with_strategy_basic` (cause unread)
- `transfer::copy_strategy::tests::test_copy_file_with_strategy_empty_file` (cause unread)
- `transfer::cross_type_policy_tests::a_refused_cross_type_item_reports_as_skipped_not_copied`
- `transfer::cross_type_policy_tests::a_refused_item_is_counted_against_its_own_kind_not_the_leaf_total`
- `transfer::cross_type_policy_tests::a_skipped_folder_with_subfolders_finishes_and_leaves_the_file`
- `transfer::cross_type_policy_tests::a_source_that_partly_landed_still_reports_done`
- `transfer::cross_type_policy_tests::an_answered_overwrite_all_carries_to_the_next_file_a_folder_lands_on`
- `transfer::cross_type_policy_tests::an_answered_overwrite_all_carries_to_the_next_folder_a_file_lands_on`
- `transfer::cross_type_policy_tests::an_answered_overwrite_older_never_deletes_the_file_it_cannot_compare`
- `transfer::cross_type_policy_tests::an_answered_overwrite_smaller_never_deletes_the_folder_it_cannot_compare`
- `transfer::cross_type_policy_tests::an_explicitly_answered_overwrite_still_replaces_a_folder_with_a_file`
- `transfer::cross_type_policy_tests::an_overwrite_all_answered_on_a_plain_clash_never_carries_across_types`
- `transfer::cross_type_policy_tests::one_skip_answers_for_the_whole_incoming_folder`
- `transfer::cross_type_policy_tests::overwrite_all_never_deletes_a_file_a_folder_landed_on`
- `transfer::cross_type_policy_tests::overwrite_all_never_deletes_a_folder_a_file_landed_on`
- `transfer::cross_type_policy_tests::overwrite_older_never_deletes_a_folder_a_file_landed_on`
- `transfer::cross_type_policy_tests::overwrite_smaller_never_deletes_a_file_a_folder_landed_on`
- `transfer::cross_type_policy_tests::overwrite_smaller_never_deletes_a_folder_a_file_landed_on`
- `transfer::folder_over_file_rollback_tests::a_completed_folder_over_file_overwrite_replaces_the_file_and_drops_the_aside`
- `transfer::folder_over_file_rollback_tests::a_rollback_mid_folder_over_file_overwrite_puts_the_file_back`
- `transfer::folder_over_file_rollback_tests::a_stopped_folder_over_file_overwrite_keeps_the_users_file_beside_the_folder`
  (cause unread)
- `transfer::hardlink_progress_tests::copy_counts_write_footprint_for_hardlinks`
- `transfer::move_op::move_interruption_tests::a_cancelled_cross_fs_move_takes_its_staging_folder_with_it`
- `transfer::move_op::move_interruption_tests::a_paused_folder_merge_stops_renaming_its_children`
- `transfer::move_op::move_interruption_tests::a_rollback_before_the_sweep_starts_reports_every_original_still_in_place`
  (cause unread)
- `transfer::move_op::move_interruption_tests::a_rollback_clicked_as_the_last_item_lands_puts_the_move_back`
- `transfer::move_op::move_interruption_tests::a_rollback_during_the_source_sweep_reports_the_originals_it_did_not_reach`
- `transfer::move_op::move_interruption_tests::the_sweeps_account_counts_a_skipped_source_as_still_in_place` (cause
  unread)
- `transfer::move_op::move_ledger_tests::a_merged_move_records_every_child_it_renamed`
- `transfer::move_op::move_ledger_tests::reversing_a_move_drains_its_ledger` (cause unread)
- `transfer::move_op::move_mac_only_tests::a_copy_that_includes_mac_only_files_writes_them_like_any_other`
- `transfer::move_op::move_mac_only_tests::a_copy_that_skips_mac_only_files_writes_and_counts_none_of_them`
- `transfer::move_op::move_progress_tests::cross_fs_local_move_emits_flushing_phase_before_complete`
- `transfer::move_op::move_progress_tests::cross_fs_local_move_reports_the_source_deletion_instead_of_sitting_at_full`
- `transfer::move_op::move_progress_tests::cross_fs_local_move_single_file_reaches_files_done_n`
- `transfer::move_op::move_progress_tests::same_fs_local_move_emits_flushing_phase_before_complete`
- `transfer::move_op::move_race_tests::a_landing_on_a_free_name_lands_when_the_name_really_is_free`
- `transfer::move_op::move_race_tests::a_landing_on_a_free_name_refuses_a_file_that_appeared_in_the_window`
- `transfer::move_op::move_reversal_tests::a_case_only_rename_back_is_not_a_collision` (cause unread)
- `transfer::move_op::move_reversal_tests::a_restore_rename_lands_when_the_original_source_is_clear` (cause unread)
- `transfer::move_op::move_reversal_tests::a_restore_rename_refuses_to_replace_an_entry_that_appeared_after_the_check`
  (cause unread)
- `transfer::move_op::move_reversal_tests::a_rollback_after_a_merge_recreates_the_source_folders_and_brings_the_children_home`
- `transfer::move_op::move_reversal_tests::an_item_replaced_at_the_destination_is_not_renamed_back` (cause unread)
- `transfer::move_op::move_reversal_tests::an_untouched_move_restores_every_item` (cause unread)
- `transfer::move_op::move_source_drift_tests::a_top_level_file_saved_over_after_its_copy_survives_the_sweep`
- `transfer::move_op::move_vanished_tests::a_destination_that_stayed_lets_the_move_finish`
- `transfer::move_op::safety_matrix_tests::matrix_dir_overwrite_older_skips_newer_child_preserves_source` (cause unread)
- `transfer::move_op::safety_matrix_tests::matrix_dir_overwrite_smaller_skips_equal_child_preserves_source` (cause
  unread)
- `transfer::move_op::safety_matrix_tests::matrix_single_file_overwrite_older_strict_semantics` (cause unread)
- `transfer::move_op::safety_matrix_tests::matrix_single_file_overwrite_replaces_dest_both_move_kinds` (cause unread)
- `transfer::move_op::safety_matrix_tests::matrix_single_file_overwrite_smaller_strict_semantics` (cause unread)
- `transfer::move_op::safety_matrix_tests::matrix_single_file_rename_keeps_both_both_move_kinds` (cause unread)
- `transfer::move_op::safety_matrix_tests::matrix_single_file_skip_preserves_source_both_move_kinds`
- `transfer::move_op::tests::a_cross_fs_move_reports_removal_only_after_the_source_delete_phase`
- `transfer::move_op::tests::a_cross_fs_move_that_skipped_the_item_ends_on_skipped_not_done`
- `transfer::move_op::tests::a_cross_fs_move_that_skipped_the_item_never_reports_it_removed`
- `transfer::move_op::tests::a_cross_fs_move_that_took_the_item_ends_on_done`
- `transfer::move_op::tests::a_local_move_never_acts_on_a_preview_of_a_different_selection`
- `transfer::move_op::tests::a_same_fs_move_that_took_the_whole_item_reports_it_removed`
- `transfer::move_op::tests::cross_fs_local_move_flushes_final_dests_before_deleting_sources`
- `transfer::move_op::tests::cross_fs_move_skip_preserves_source_and_dest`
- `transfer::named_leaves_engine_tests::a_clash_at_a_renamed_leaf_meets_the_policy_at_the_renamed_path`
- `transfer::named_leaves_engine_tests::an_interrupted_named_run_leaves_nothing_half_named` (cause unread)
- `transfer::staged_write::tests::cut_off::a_cancel_during_a_cut_off_landings_byte_compare_stops_it_and_keeps_the_landing_recorded`
  (cause unread)
- `transfer::staged_write::tests::ledger::a_claimed_name_landing_is_on_disk_as_a_landing_before_it_clears_the_way`
  (cause unread)
- `transfer::staged_write::tests::ledger::a_landing_cut_off_after_clearing_the_way_is_finished_by_the_next_launch`
  (cause unread)
- `transfer::staged_write::tests::ledger::a_swap_whose_aside_record_has_no_writer_leaves_the_original_in_place` (cause
  unread)
- `transfer::staged_write::tests::ledger::a_swap_with_no_path_space_for_its_aside_record_leaves_the_original_in_place`
  (cause unread)
- `transfer::transaction_integration_test::a_destination_changed_since_the_copy_wrote_it_survives_the_reversal` (cause
  unread)
- `transfer::transaction_integration_test::test_copy_transaction_rollback_mixed` (cause unread)
- `transfer::transaction_integration_test::test_copy_transaction_rollback_removes_files` (cause unread)
- `transfer::type_mismatch_rename_tests::file_over_folder_rename_escalates_past_existing_numbered_name`
- `transfer::type_mismatch_rename_tests::file_over_folder_rename_keeps_existing_folder_lands_file_with_bytes`
- `transfer::type_mismatch_rename_tests::folder_over_file_rename_escalates_past_existing_numbered_name`
- `transfer::type_mismatch_rename_tests::folder_over_file_rename_keeps_existing_file_lands_folder_renamed`
- `transfer::type_mismatch_rename_tests::move_file_over_folder_rename_keeps_existing_folder_lands_file` (cause unread)
- `transfer::type_mismatch_rename_tests::move_folder_over_file_rename_keeps_existing_file_lands_folder_renamed` (cause
  unread)
- `transfer::volume::copy::crashsafe_tests::local_staging::a_non_overwrite_local_copy_refuses_to_clobber_a_destination_that_appeared`
- `transfer::volume::copy::crashsafe_tests::local_staging::an_abandoned_local_copy_leaves_a_cmdr_temp_and_no_file_at_the_real_name`
- `transfer::volume::swap_outage_tests::a_drop_at_the_rename_after_the_move_aside_resumes_and_lands_at_every_site`
  (cause unread)
- `transfer::volume::swap_outage_tests::a_drop_before_a_one_step_replace_waits_and_lands_at_every_site` (cause unread)
- `transfer::volume::swap_outage_tests::a_drop_before_the_original_moves_aside_waits_and_lands_at_every_site` (cause
  unread)
- `transfer::volume::swap_outage_tests::a_move_aside_whose_reply_was_lost_resumes_the_landing_without_streaming_again_at_every_site`
  (cause unread)
- `transfer::volume::swap_outage_tests::a_one_step_replace_whose_reply_was_lost_counts_as_landed_only_on_the_sources_bytes`
  (cause unread)
- `transfer::volume::swap_outage_tests::a_rename_whose_reply_was_lost_counts_as_landed_only_on_the_sources_bytes` (cause
  unread)
- `transfer::volume::swap_outage_tests::a_stale_error_on_the_move_aside_re_runs_the_swap_at_every_site` (cause unread)
- `transfer::volume::swap_outage_tests::a_temp_gone_with_the_old_bytes_at_the_name_is_not_landed_at_every_site` (cause
  unread)
- `unique_name::tests::a_dangling_symlink_counts_as_taken` (cause unread)

### Not this plan: mounted-inside and node identity: Windows plan task 35 (32)

- `delete::already_gone_tests::a_local_delete_carries_on_past_a_file_that_vanished_after_the_scan`
- `delete::hardlink_progress_tests::delete_hardlinked_files_does_not_overshoot_progress`
- `delete::preview_binding_tests::a_local_delete_never_acts_on_a_preview_covering_more_than_was_asked_for`
- `delete::preview_binding_tests::a_local_delete_never_acts_on_a_preview_of_a_different_selection`
- `delete::preview_binding_tests::a_local_delete_never_acts_on_a_preview_walked_for_a_copy` (cause unread)
- `delete::trash::tests::test_move_to_trash_sync_nonexistent` (cause unread)
- `journal_capture_tests::a_finished_same_fs_move_reports_the_items_it_moved`
- `journal_capture_tests::same_fs_move_journals_the_top_level_item_as_rollback_unit`
- `journal_row_shape_tests::a_folder_delete_journals_one_row_per_leaf_at_its_original_path`
- `ledger::tests::a_local_directory_records_its_node_and_no_size`
- `ledger::tests::a_local_file_records_its_size_and_node`
- `ledger::tests::a_same_size_replacement_is_a_different_identity`
- `ledger::tests::a_symlink_records_the_link_not_its_target` (cause unread)
- `ledger::tests::copy_transaction_rollback_deletes_files_and_dirs_in_reverse` (cause unread)
- `transfer::hardlink_progress_tests::move_hardlinked_files_does_not_overshoot_progress`
- `transfer::move_op::move_mac_only_tests::a_cross_fs_move_that_skips_mac_only_files_leaves_no_husk_in_the_source`
- `transfer::move_op::move_mac_only_tests::a_source_folder_kept_by_a_newcomer_keeps_its_mac_only_files`
- `transfer::move_op::move_source_drift_tests::an_original_saved_over_after_its_copy_survives_the_sweep`
- `transfer::move_op::move_source_sweep_tests::a_clean_move_removes_every_source_and_reports_nothing_left_behind`
- `transfer::move_op::move_source_sweep_tests::a_file_that_appeared_after_the_scan_survives_the_source_sweep`
- `transfer::move_op::move_source_sweep_tests::a_skip_while_staging_keeps_the_original`
- `transfer::move_op::move_source_sweep_tests::a_source_dir_that_kept_a_skipped_child_never_reports_removal`
- `transfer::move_op::move_source_sweep_tests::a_source_that_kept_a_newcomer_ends_on_skipped_and_never_reports_removal`
- `transfer::move_op::move_source_sweep_tests::a_symlinked_source_file_is_removed_as_a_link_and_its_target_is_untouched`
  (cause unread)
- `transfer::move_op::move_source_sweep_tests::the_completion_event_counts_what_stayed_behind`
- `transfer::move_op::named_move_tests::named_local_folder_moves_preserve_children_and_empty_directories`
- `transfer::move_op::named_move_tests::named_local_moves_use_the_requested_leaf_and_preserve_skipped_originals` (cause
  unread)
- `transfer::move_op::tests::a_cross_fs_merge_that_preserved_a_skipped_child_reports_the_source_still_there`
- `transfer::move_op::tests::a_same_fs_move_shows_one_row_per_item_settled_by_its_rename`
- `transfer::move_op::tests::cross_fs_move_dir_merge_skip_child_preserves_source_child`
- `transfer::move_op::tests::cross_fs_move_keeps_folder_dates`
- `transfer::move_op::tests::cross_fs_move_preserves_empty_directories`

### S, `std::path` arithmetic on an app path: this crate's app path conversion (37)

- `archive_edit::driver_tests::deleting_inside_a_remote_archive_reports_its_upload`
- `archive_edit::driver_tests::route_archive_delete_removes_entries_and_completes` (cause unread)
- `archive_edit::driver_tests::route_archive_delete_reports_the_deleted_count_not_the_retained_count` (cause unread)
- `archive_edit::fresh_plan::outage_tests::a_listing_the_server_refuses_fails_the_planning_walk_at_once`
- `archive_edit::fresh_plan::outage_tests::a_planning_walk_whose_share_stays_gone_says_it_lost_the_connection`
- `journal_capture_volume_tests::an_interrupted_concurrent_volume_copy_journals_every_child_it_finished`
- `journal_capture_volume_tests::an_interrupted_volume_folder_copy_journals_every_child_it_finished`
- `journal_capture_volume_tests::undoing_a_copy_removes_the_links_it_landed`
- `transfer::volume::copy::look_alike_tests::a_look_alike_prompts_about_the_stored_entry`
- `transfer::volume::copy::look_alike_tests::a_top_level_look_alike_prompt_says_it_is_one`
- `transfer::volume::copy::merge_case_fold_tests::an_unanswerable_probe_fails_the_item_instead_of_writing`
- `transfer::volume::copy::outage_tests::a_copy_that_outlasts_its_outage_says_it_lost_the_connection`
- `transfer::volume::copy::outage_tests::a_deep_clash_that_outlasts_its_outage_says_it_lost_the_connection`
- `transfer::volume::copy::precheck_tests::a_destination_name_differing_only_in_case_is_still_a_conflict`
- `transfer::volume::copy::precheck_tests::a_failed_listing_falls_back_to_per_file_probes`
- `transfer::volume::failure_side_tests::a_concurrent_copy_reports_a_refused_file_write_as_the_destinations`
- `transfer::volume::failure_side_tests::a_concurrent_copy_reports_a_refused_landing_as_the_destinations`
- `transfer::volume::failure_side_tests::a_concurrent_copy_reports_a_refused_nested_folder_as_the_destinations`
- `transfer::volume::failure_side_tests::a_concurrent_copy_reports_an_unreadable_source_file_as_the_sources`
- `transfer::volume::failure_side_tests::a_cross_volume_move_reports_a_refused_file_write_as_the_destinations`
- `transfer::volume::failure_side_tests::a_cross_volume_move_reports_a_refused_landing_as_the_destinations`
- `transfer::volume::failure_side_tests::a_cross_volume_move_reports_a_refused_nested_folder_as_the_destinations`
- `transfer::volume::failure_side_tests::a_cross_volume_move_reports_an_unopenable_source_file_as_the_sources`
- `transfer::volume::failure_side_tests::a_cross_volume_move_reports_an_unreadable_source_file_as_the_sources`
- `transfer::volume::failure_side_tests::a_serial_copy_reports_a_refused_file_write_as_the_destinations`
- `transfer::volume::failure_side_tests::a_serial_copy_reports_a_refused_landing_as_the_destinations`
- `transfer::volume::failure_side_tests::a_serial_copy_reports_a_refused_nested_folder_as_the_destinations`
- `transfer::volume::failure_side_tests::a_serial_copy_reports_an_unopenable_source_file_as_the_sources`
- `transfer::volume::failure_side_tests::a_serial_copy_reports_an_unreadable_source_file_as_the_sources`
- `transfer::volume::kept_new_data_tests::a_failure_that_names_its_own_rescue_lists_only_the_others`
- `transfer::volume::preflight_outage_tests::a_scan_whose_share_stays_gone_says_it_lost_the_connection`
- `transfer::volume::safe_replace_grid_tests::a_concurrent_task_dropped_mid_swap_keeps_the_original_for_the_sweep_to_put_back`
  (cause unread)
- `transfer::volume::safe_replace_grid_tests::a_name_that_takes_nothing_rescues_the_new_bytes_and_keeps_the_original_aside_at_every_site`
- `transfer::volume::safe_replace_grid_tests::a_refused_landing_puts_the_original_back_at_every_site`
- `transfer::volume::safe_replace_grid_tests::a_refused_move_aside_of_the_original_keeps_it_and_takes_the_temp_away_at_every_site`
- `transfer::volume::safe_replace_grid_tests::every_site_lands_the_new_bytes_at_the_originals_name`
- `transfer_tree::build::tests::twin_sources_hang_off_their_real_folder_under_their_plain_names`

### Timed out: this crate's app path conversion (3)

- `archive_edit::copy_into_remote_tests::move_into_from_a_remote_source_keeps_originals_when_a_collision_is_skipped`
- `scan_pause_tests::an_operations_own_scan_parks_too`
- `transfer::move_op::move_interruption_tests::a_paused_move_stops_renaming_until_it_resumes`

## cmdr-restic (67)

### S, `std::path` arithmetic on an app path: this crate's app path conversion (58)

- `hub::lifecycle_tests::a_deleted_repo_answers_not_found_not_an_empty_list`
- `hub::lifecycle_tests::a_load_reports_its_bytes_as_it_reads`
- `hub::lifecycle_tests::a_new_snapshot_reloads_the_index_while_an_old_reader_finishes`
- `hub::lifecycle_tests::a_repo_replaced_in_place_opens_again_and_lists_the_new_one`
- `hub::lifecycle_tests::a_transfer_hold_keeps_the_index_past_the_idle_drop_and_another_repos_load`
- `hub::lifecycle_tests::an_idle_index_is_dropped_and_its_password_kept`
- `hub::lifecycle_tests::an_index_bigger_than_the_window_loads_reading_each_file_once`
- `hub::lifecycle_tests::an_index_in_use_is_never_evicted`
- `hub::lifecycle_tests::an_index_listed_without_sizes_loads_reading_each_file_once`
- `hub::lifecycle_tests::loading_the_index_never_writes_to_the_repo`
- `hub::lifecycle_tests::password_tests::a_store_miss_is_asked_once`
- `hub::lifecycle_tests::password_tests::a_store_that_refuses_to_forget_still_locks_the_repo`
- `hub::lifecycle_tests::password_tests::a_stored_password_opens_the_repo_without_asking`
- `hub::lifecycle_tests::password_tests::a_stored_password_that_stopped_working_asks_as_a_wrong_attempt`
- `hub::lifecycle_tests::password_tests::an_unlock_dropped_while_another_open_runs_keeps_nothing` (cause unread)
- `hub::lifecycle_tests::password_tests::an_unlock_in_flight_during_a_forget_keeps_and_files_nothing` (cause unread)
- `hub::lifecycle_tests::password_tests::forgetting_the_saved_password_locks_the_repo_until_the_next_unlock`
- `hub::lifecycle_tests::password_tests::nothing_is_kept_or_filed_for_a_cancelled_unlock_a_wrong_password_or_remember_off`
  (cause unread)
- `hub::lifecycle_tests::password_tests::only_remember_password_files_a_verified_password` (cause unread)
- `hub::lifecycle_tests::past_the_cap_the_least_recently_used_index_goes_and_its_password_stays`
- `hub::lifecycle_tests::root_listings_dont_keep_an_idle_index_loaded`
- `path::tests::a_dated_folder_classifies_by_its_short_id_alone` (cause unread)
- `path::tests::a_stale_time_prefix_still_classifies_by_the_id` (cause unread)
- `path::tests::latest_and_its_contents_classify_as_latest` (cause unread)
- `path::tests::the_view_root_classifies_as_the_snapshot_list` (cause unread)
- `read_stream::tests::the_chunks_come_out_in_order` (cause unread)
- `read_stream::tests::the_stream_reads_the_next_chunks_before_they_are_asked_for` (cause unread)
- `tree::tests::a_file_lists_with_its_size_date_mode_and_owner` (cause unread)
- `volume::stale_index_tests::a_pack_missing_after_a_fresh_load_is_a_real_not_found`
- `volume::stale_index_tests::a_range_read_after_a_prune_elsewhere_reloads_the_index`
- `volume::stale_index_tests::a_stream_opened_before_a_prune_elsewhere_reads_on_through_a_reload`
- `volume::stale_index_tests::after_a_prune_elsewhere_a_listing_and_a_read_reload_the_index_once`
- `volume::tests::a_batch_scan_asks_the_boundary_inside_the_walk` (cause unread)
- `volume::tests::a_batch_scan_stops_when_it_is_told_to` (cause unread)
- `volume::tests::a_cancelled_listing_answers_cancelled` (cause unread)
- `volume::tests::a_file_streams_its_bytes_date_and_mode` (cause unread)
- `volume::tests::a_folder_or_a_link_has_no_bytes_to_stream` (cause unread)
- `volume::tests::a_link_reports_the_target_the_backup_recorded` (cause unread)
- `volume::tests::a_name_that_names_no_snapshot_is_not_found` (cause unread)
- `volume::tests::a_paused_copy_out_of_latest_keeps_it_until_the_copy_settles` (cause unread)
- `volume::tests::a_replaced_parent_is_read_through_from_then_on` (cause unread)
- `volume::tests::a_snapshot_row_carries_its_host_tags_and_paths`
- `volume::tests::a_snapshot_tree_lists_files_folders_links_and_odd_names` (cause unread)
- `volume::tests::a_snapshot_without_a_summary_lists_without_a_size`
- `volume::tests::a_stale_time_prefix_still_finds_its_snapshot` (cause unread)
- `volume::tests::a_wrong_password_unlocks_nothing_and_keeps_nothing`
- `volume::tests::browsing_and_reading_never_write_to_the_repo` (cause unread)
- `volume::tests::is_writable_honors_the_shared_declaration_contract` (cause unread)
- `volume::tests::it_answers_as_a_read_only_view_routed_over_its_parent` (cause unread)
- `volume::tests::latest_doesnt_switch_under_a_running_copy` (cause unread)
- `volume::tests::latest_moves_on_once_nothing_reads_under_it` (cause unread)
- `volume::tests::listing_dates_honor_the_shared_claim_contract` (cause unread)
- `volume::tests::metadata_answers_for_the_view_a_snapshot_and_a_file` (cause unread)
- `volume::tests::not_found_honors_the_shared_path_payload_contract` (cause unread)
- `volume::tests::ranged_reads_fetch_windows_of_the_pack_never_the_whole_pack` (cause unread)
- `volume::tests::refreshing_the_root_alone_never_holds_latest_back` (cause unread)
- `volume::tests::the_view_lists_one_folder_per_snapshot_plus_latest` (cause unread)
- `volume::tests::without_a_password_the_view_asks_for_one`

### Timed out: this crate's app path conversion (9)

- `hub::lifecycle_tests::a_cancel_during_the_index_listing_lets_the_listing_finish`
- `hub::lifecycle_tests::a_refresh_after_a_cancelled_stuck_one_waits_for_it_instead_of_piling_up`
- `hub::lifecycle_tests::a_snapshot_landing_during_a_refresh_shows_in_the_listing_that_asked_after`
- `hub::lifecycle_tests::a_waiter_leaving_keeps_the_load_alive_for_the_others`
- `hub::lifecycle_tests::concurrent_listings_share_one_index_load`
- `hub::lifecycle_tests::password_tests::a_forget_answers_at_once_and_an_open_in_flight_keeps_nothing`
- `hub::lifecycle_tests::password_tests::a_forget_cancels_an_index_load_in_flight_and_it_installs_nothing`
- `hub::lifecycle_tests::root_listings_that_ask_during_a_refresh_share_the_next_one`
- `hub::lifecycle_tests::the_last_waiter_leaving_cancels_the_load_and_the_next_listing_starts_fresh`

## cmdr-s3 (48)

### Not this plan: a Windows-only test expectation: #1232 and the Windows build state (1)

- `metadata::metadata_test::nanoseconds_survive_and_trailing_zeros_drop`

### S, `std::path` arithmetic on an app path: this crate's app path conversion (42)

- `volume::abort_confirm_test::a_failed_upload_answers_before_its_abort_is_confirmed`
- `volume::batch_retry_test::a_throttled_batch_delete_goes_again_and_lands` (cause unread)
- `volume::copy_landed_test::a_one_request_copy_keeps_the_sources_date_beside_its_own_token`
- `volume::copy_landed_test::a_one_request_copy_whose_answer_never_comes_reports_the_copy_it_published` (cause unread)
- `volume::copy_landed_test::a_source_replaced_after_its_head_is_source_changed_and_copies_nothing`
- `volume::copy_landed_test::an_r2_copy_onto_a_taken_name_is_already_exists`
- `volume::cut_off_record_test::the_next_write_to_the_key_clears_a_cut_off_body_and_lands`
- `volume::ended_upload_test::a_create_new_whose_upload_another_writer_ended_is_refused_as_taken`
- `volume::folder_delete_test::a_folder_delete_never_takes_a_file` (cause unread)
- `volume::folder_delete_test::a_folder_delete_takes_an_empty_folders_marker` (cause unread)
- `volume::folder_delete_test::a_fresh_volume_deletes_a_markerless_folder_without_the_file_of_its_name` (cause unread)
- `volume::folder_delete_test::a_listed_markerless_folder_deletes_without_the_hidden_file_of_its_name`
- `volume::folder_delete_test::the_folder_row_beside_a_hidden_file_is_only_the_folder` (cause unread)
- `volume::fresh_folder_test::a_server_copy_into_a_fresh_folder_sends_no_head_at_its_destination`
- `volume::fresh_folder_test::an_upload_into_a_fresh_folder_sends_no_no_overwrite_head`
- `volume::get_length_test::a_ranged_read_that_arrives_whole_ends_cleanly`
- `volume::get_length_test::a_ranged_read_that_ends_before_its_named_length_fails`
- `volume::get_length_test::a_read_without_a_length_declares_the_callers_hint`
- `volume::get_length_test::a_read_without_a_length_or_a_hint_says_it_doesnt_know`
- `volume::late_cancel_test::a_buffered_put_whose_answer_never_comes_keeps_the_overwrite_it_finished`
- `volume::late_cancel_test::a_cancel_after_the_whole_body_went_out_keeps_the_new_file_it_finished`
- `volume::late_cancel_test::a_cancel_after_the_whole_body_went_out_keeps_the_overwrite_it_finished`
- `volume::late_cancel_test::a_cancel_before_the_last_piece_still_publishes_nothing`
- `volume::late_cancel_test::a_completion_whose_answer_never_comes_reports_the_file_it_published`
- `volume::late_cancel_test::a_copy_whose_completion_answer_never_comes_reports_the_copy_it_published`
- `volume::late_cancel_test::a_put_whose_answer_never_comes_keeps_the_overwrite_it_finished`
- `volume::late_cancel_test::a_truncated_body_a_server_kept_is_still_removed`
- `volume::long_key_test::a_key_at_the_ceiling_stats_tallies_renames_and_deletes` (cause unread)
- `volume::long_key_test::a_missing_key_at_the_ceiling_is_not_found` (cause unread)
- `volume::paths::paths_test::a_bare_server_path_is_refused` (cause unread)
- `volume::paths::paths_test::a_bucket_place_is_rooted_at_its_bucket`
- `volume::paths::paths_test::the_account_root_reaches_every_bucket`
- `volume::put_retry_test::a_fault_after_the_publish_reports_the_write`
- `volume::put_retry_test::a_faulted_put_goes_again_and_lands`
- `volume::put_retry_test::a_put_still_faulted_after_its_retries_fails_and_stores_nothing` (cause unread)
- `volume::put_source_test::a_failing_source_ends_the_write_with_its_own_error`
- `volume::put_source_test::a_source_longer_than_promised_sends_nothing`
- `volume::put_source_test::a_source_shorter_than_promised_sends_nothing`
- `volume::refused_name_test::b2_refuses_a_control_character_before_sending_anything`
- `volume::refused_name_test::gcs_refuses_a_line_break_before_sending_anything`
- `volume::refused_name_test::r2_stores_a_tab_in_a_name`
- `volume::share_link_test::a_place_with_no_live_client_says_it_is_disconnected`

### Timed out: this crate's app path conversion (5)

- `volume::pause_test::a_cancel_while_paused_ends_the_upload_and_leaves_nothing`
- `volume::pause_test::a_put_paused_mid_body_is_set_aside_and_sent_again_whole`
- `volume::pause_test::a_put_waits_while_paused_and_lands_on_resume`
- `volume::pause_test::parts_paused_mid_body_are_set_aside_and_sent_again_on_resume`
- `volume::pause_test::parts_wait_while_paused_and_finish_on_resume`

## cmdr-sftp (7)

### S, `std::path` arithmetic on an app path: this crate's app path conversion (7)

- `volume::paths::paths_test::a_bare_server_absolute_path_is_refused` (cause unread)
- `volume::paths::paths_test::a_path_to_an_existing_entry_keeps_its_exact_bytes`
- `volume::paths::paths_test::a_prefixed_and_a_relative_path_land_on_the_same_server_path_and_round_trip`
- `volume::paths::paths_test::a_relative_path_hangs_off_the_root`
- `volume::paths::paths_test::a_volume_rooted_at_the_server_root_reaches_everything`
- `volume::paths::paths_test::the_three_spellings_of_the_root_all_mean_the_root`
- `volume::sharing_test::an_instance_sharing_the_connection_answers_to_its_own_name_and_root`

## cmdr-smb (13)

### S, `std::path` arithmetic on an app path: this crate's app path conversion (13)

- `volume::paths::paths_test::a_mixed_form_path_reaches_the_wire_byte_for_byte`
- `volume::paths::paths_test::a_root_anchored_dialog_destination_reaches_the_share`
- `volume::paths::paths_test::an_anchored_mount_joins_its_share_root_onto_the_wire_path` (cause unread)
- `volume::paths::paths_test::an_anchored_mount_keeps_the_bytes_below_its_anchor` (cause unread)
- `volume::paths::paths_test::an_anchored_mount_root_is_the_share_root_itself` (cause unread)
- `volume::paths::paths_test::an_anchored_mount_still_rejects_a_path_outside_its_mount` (cause unread)
- `volume::paths::paths_test::the_two_directions_round_trip_on_an_anchored_mount` (cause unread)
- `volume::paths::paths_test::to_smb_path_absolute_under_mount` (cause unread)
- `volume::paths::paths_test::to_smb_path_mount_root` (cause unread)
- `volume::paths::paths_test::to_smb_path_rejects_a_path_outside_the_mount` (cause unread)
- `volume::paths::paths_test::to_smb_path_rejects_a_sibling_mount_that_merely_shares_a_name_prefix` (cause unread)
- `volume::volume_impl::volume_impl_test::a_promotion_uses_the_anchor_recorded_for_the_target_root` (cause unread)
- `volume::volume_impl::volume_impl_test::a_rerooted_share_translates_paths_under_its_new_mount` (cause unread)

## cmdr-viewer (13)

### Cause unread: this crate's app path conversion (13)

- `markdown_images_test::a_document_inside_an_archive_resolves_inside_it` (cause unread)
- `markdown_images_test::a_document_on_a_phone_resolves_on_the_phone` (cause unread)
- `markdown_images_test::a_relative_source_drops_its_query_and_fragment_and_decodes_escapes` (cause unread)
- `markdown_images_test::a_relative_source_resolves_beside_the_document` (cause unread)
- `markdown_images_test::climbing_past_the_volume_root_stops_at_the_root` (cause unread)
- `markdown_links_test::a_document_on_a_phone_or_in_an_archive_links_within_it` (cause unread)
- `markdown_links_test::a_fragment_and_a_path_decode_their_escapes_and_the_query_goes` (cause unread)
- `markdown_links_test::a_link_to_the_document_itself_is_an_anchor` (cause unread)
- `markdown_links_test::a_relative_link_keeps_its_fragment_as_the_rendered_heading_id` (cause unread)
- `markdown_links_test::a_relative_link_opens_the_file_beside_the_document` (cause unread)
- `markdown_links_test::climbing_past_the_volume_root_stops_at_the_root` (cause unread)
- `markdown_links_test::the_browser_url_conversions_answer_what_url_does` (cause unread)
- `wire_characterization_test::a_markdown_session_renders_and_resolves_links_in_these_shapes` (cause unread)

## cmdr-webdav (23)

### S, `std::path` arithmetic on an app path: this crate's app path conversion (23)

- `volume::delete_aside_test::a_file_landing_after_the_check_is_kept_and_the_delete_refused` (cause unread)
- `volume::delete_aside_test::a_folder_whose_name_was_taken_meanwhile_is_kept_beside_it` (cause unread)
- `volume::delete_aside_test::a_forced_rename_falls_back_when_the_aside_move_is_refused`
- `volume::delete_aside_test::a_forced_rename_keeps_a_file_landing_in_the_folder_it_replaces` (cause unread)
- `volume::delete_aside_test::a_forced_rename_replaces_an_empty_folder_and_leaves_nothing_behind`
- `volume::delete_aside_test::a_forced_rename_whose_destination_is_retaken_keeps_everything` (cause unread)
- `volume::delete_aside_test::a_kept_name_already_taken_gets_a_number` (cause unread)
- `volume::delete_aside_test::a_listing_tidies_an_empty_leftover_aside_and_nothing_else` (cause unread)
- `volume::delete_aside_test::a_server_that_refuses_the_move_falls_back_to_a_plain_delete`
- `volume::delete_aside_test::an_empty_folder_is_deleted_and_leaves_nothing_behind`
- `volume::get_length_test::a_ranged_read_that_arrives_whole_ends_cleanly`
- `volume::get_length_test::a_ranged_read_that_ends_before_its_named_length_fails`
- `volume::get_length_test::a_read_without_a_length_declares_the_callers_hint`
- `volume::get_length_test::a_read_without_a_length_or_a_hint_says_it_doesnt_know`
- `volume::paths::paths_test::a_bare_remote_path_is_refused` (cause unread)
- `volume::paths::paths_test::a_dot_dot_escape_is_refused_however_it_is_spelled`
- `volume::paths::paths_test::a_path_to_an_existing_entry_keeps_its_exact_bytes`
- `volume::paths::paths_test::a_prefixed_and_a_relative_path_land_on_the_same_remote_path_and_round_trip`
- `volume::paths::paths_test::every_spelling_of_the_root_is_the_root`
- `volume::paths::paths_test::the_root_of_a_volume_normalizes_to_one_spelling` (cause unread)
- `volume::sharing_test::an_instance_sharing_the_client_answers_to_its_own_name_and_root`
- `volume::slow_server_test::a_body_that_keeps_moving_is_never_cut`
- `volume::slow_server_test::a_listing_a_busy_server_takes_its_time_over_is_waited_for` (cause unread)
