//! Shared request engine and collection runner. Enable `desktop` for the native UI.
#[cfg(feature = "desktop")]
mod ai_ui;
#[cfg(feature = "desktop")]
mod app;
#[cfg(feature = "desktop")]
mod app_ui;
mod auth;
mod auth_migration;
#[cfg(feature = "desktop")]
mod auth_migration_ui;
mod auth_vault;
#[cfg(all(test, feature = "desktop"))]
mod autosave_tests;
#[cfg(feature = "desktop")]
mod benchmark;
#[cfg(feature = "desktop")]
mod body_ui;
mod bru;
pub mod cancellation;
mod certificate_keys;
mod cli;
#[cfg(feature = "desktop")]
mod collection_lifecycle;
#[cfg(all(test, feature = "desktop"))]
mod collection_lifecycle_tests;
pub mod collection_runner;
#[cfg(feature = "desktop")]
mod collection_settings;
#[cfg(feature = "desktop")]
mod collection_ui;
mod command_import;
mod cookies;
#[cfg(feature = "desktop")]
mod cookies_ui;
#[cfg(feature = "desktop")]
mod curl_editor;
#[cfg(feature = "desktop")]
mod devtools;
#[cfg(feature = "desktop")]
mod dialogs;
mod disk_sync;
#[cfg(feature = "desktop")]
mod editors;
pub mod engine;
#[cfg(feature = "desktop")]
mod environment_rows;
#[cfg(feature = "desktop")]
mod environment_tabs;
#[cfg(feature = "desktop")]
mod environment_tree;
#[cfg(all(test, feature = "desktop"))]
mod environment_tabs_tests;
mod environment_vault;
mod environments;
mod file_cache;
#[cfg(feature = "desktop")]
mod file_explorer;
#[cfg(feature = "desktop")]
mod folder_settings;
mod generated_headers;
#[cfg(feature = "desktop")]
mod git;
#[cfg(feature = "desktop")]
mod git_commit_ui;
#[cfg(feature = "desktop")]
mod git_dialogs;
#[cfg(feature = "desktop")]
mod git_diff;
#[cfg(feature = "desktop")]
mod git_diff_ui;
#[cfg(feature = "desktop")]
mod git_graph;
#[cfg(feature = "desktop")]
mod git_log_ui;
#[cfg(feature = "desktop")]
mod git_ops;
#[cfg(feature = "desktop")]
mod git_review;
#[cfg(feature = "desktop")]
mod git_ui;
#[cfg(all(test, feature = "desktop"))]
mod git_workflow_tests;
mod grpc;
mod grpc_message;
#[cfg(feature = "desktop")]
mod grpc_picker;
mod grpc_tls;
#[cfg(feature = "desktop")]
mod home;
mod http_request;
#[cfg(feature = "desktop")]
mod icons;
mod ide_bridge;
mod import;
mod inheritance;
mod integrations;
mod interchange;
mod json_comments;
#[cfg(feature = "desktop")]
mod keybindings;
#[cfg(feature = "desktop")]
mod markdown;
#[cfg(feature = "desktop")]
mod menu;
mod migration;
mod mock;
pub mod model;
mod native_format;
#[cfg(feature = "desktop")]
mod navigation;
mod network;
#[cfg(feature = "desktop")]
mod new_request;
mod ntlm;
mod oauth_browser;
mod openapi;
#[cfg(feature = "desktop")]
mod panes;
#[cfg(feature = "desktop")]
mod persistence;
mod postman;
mod preference_model;
mod preference_network;
#[cfg(test)]
mod preference_tests;
#[cfg(feature = "desktop")]
mod preferences;
#[cfg(feature = "desktop")]
mod preferences_appearance;
#[cfg(feature = "desktop")]
mod preferences_connection;
#[cfg(feature = "desktop")]
mod preferences_general;
#[cfg(feature = "desktop")]
mod preferences_integrations;
#[cfg(feature = "desktop")]
mod protocol_ui;
mod query;
#[cfg(feature = "desktop")]
mod request_header;
#[cfg(all(test, feature = "desktop"))]
mod request_order_tests;
#[cfg(feature = "desktop")]
mod request_settings;
#[cfg(feature = "desktop")]
mod request_tabs;
#[cfg(feature = "desktop")]
mod runner;
pub mod runner_data;
mod runner_report;
#[cfg(feature = "desktop")]
mod script_effects;
mod scripts;
#[cfg(all(test, feature = "desktop"))]
mod scrollbar_style_tests;
mod secrets;
pub mod storage;
#[cfg(all(test, feature = "desktop"))]
mod table_edit_tests;
#[cfg(feature = "desktop")]
mod terminal;
#[cfg(feature = "desktop")]
mod theme;
#[cfg(feature = "desktop")]
mod theme_registry;
mod tree;
#[cfg(feature = "desktop")]
mod tree_drag;
#[cfg(feature = "desktop")]
mod tree_menu;
mod tree_move;
#[cfg(feature = "desktop")]
mod tree_ops;
#[cfg(feature = "desktop")]
mod url_variables;
mod websocket;
#[cfg(feature = "desktop")]
mod widgets;
#[cfg(feature = "desktop")]
mod window_frame;
mod workspace_store;
#[cfg(test)]
mod workspace_tests;
#[cfg(feature = "desktop")]
mod workspace_ui;
mod workspace_zip;
#[cfg(feature = "desktop")]
mod workspaces;
mod yaml;

pub fn run_cli() -> i32 {
    cli::main()
}

#[cfg(feature = "desktop")]
pub use desktop::run_desktop;

#[cfg(feature = "desktop")]
mod desktop;
mod file_ids;
#[cfg(feature = "desktop")]
mod file_watch;
#[cfg(feature = "desktop")]
mod generated_headers_ui;
#[cfg(feature = "desktop")]
mod grpc_tls_ui;
#[cfg(feature = "desktop")]
mod tree_move_ui;
mod ui_state;

mod protocol_prepare;

#[cfg(feature = "desktop")]
mod mock_ui;
mod text;

mod request_protocol;
