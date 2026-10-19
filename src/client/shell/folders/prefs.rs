//! The fork's own client state, saved next to upstream's client preferences
//! in a separate file so stock Herdr can keep reading and rewriting its own.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Default, Deserialize, Serialize)]
struct ForkClientState {
    /// `grouped`, `priority` or `folders`; kept as text so a value from a
    /// newer build never makes the whole file unreadable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_panel_mode: Option<String>,
    /// Collapsed folder ids per machine (`local` or `ssh:<profile>`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    collapsed_folders: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    collapsed_machines: Vec<String>,
    /// Collapsed agents-panel workspace headers per machine.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    collapsed_agent_workspaces: BTreeMap<String, Vec<String>>,
}

/// The fork's state file for an upstream preferences file.
pub(in crate::client::shell) fn fork_state_path(
    preferences: &std::path::Path,
) -> std::path::PathBuf {
    preferences.with_extension("fork.json")
}

fn parse_mode(value: &str) -> Option<AgentPanelMode> {
    match value {
        "grouped" => Some(AgentPanelMode::Grouped),
        "priority" => Some(AgentPanelMode::Priority),
        "folders" => Some(AgentPanelMode::Folders),
        _ => None,
    }
}

fn parse_endpoint(key: &str) -> Option<ClientEndpointId> {
    if key == "local" {
        return Some(ClientEndpointId::Local);
    }
    let profile = key.strip_prefix("ssh:")?;
    crate::client::endpoint::ProfileId::parse(profile)
        .ok()
        .map(ClientEndpointId::Ssh)
}

fn load_state(path: &std::path::Path) -> ForkClientState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn by_endpoint(saved: BTreeMap<String, Vec<String>>) -> HashMap<ClientEndpointId, HashSet<String>> {
    saved
        .into_iter()
        .filter_map(|(key, ids)| Some((parse_endpoint(&key)?, ids.into_iter().collect())))
        .collect()
}

fn by_key(state: &HashMap<ClientEndpointId, HashSet<String>>) -> BTreeMap<String, Vec<String>> {
    state
        .iter()
        .filter(|(_, ids)| !ids.is_empty())
        .map(|(endpoint_id, ids)| {
            let mut ids = ids.iter().cloned().collect::<Vec<_>>();
            ids.sort();
            (endpoint_id.storage_key(), ids)
        })
        .collect()
}

/// The configured mode: the fork-only key when set, else upstream's key.
pub(in crate::client::shell) fn configured_mode(config: &ClientShellConfig) -> AgentPanelMode {
    config
        .agent_panel_mode
        .map(AgentPanelMode::from_config)
        .unwrap_or_else(|| AgentPanelMode::from_sort(config.agent_panel_sort))
}

impl FolderUiState {
    /// Reads saved fork state and settles the agents panel mode: a manual
    /// choice made in the fork wins, then upstream's saved manual choice,
    /// then config. Also points upstream's sort at the matching flat order.
    pub(in crate::client::shell) fn load(
        config: &mut ClientShellConfig,
        upstream: &preferences::ClientChromePreferences,
    ) -> (Self, HashSet<ClientEndpointId>) {
        let prefs_path = config.preferences_path.as_deref().map(fork_state_path);
        let saved = prefs_path.as_deref().map(load_state).unwrap_or_default();
        let fork_mode = saved.agent_panel_mode.as_deref().and_then(parse_mode);
        let (agent_mode, agent_mode_manual) = match (fork_mode, upstream.agent_panel_sort) {
            (Some(mode), _) => (mode, true),
            (None, Some(sort)) => (AgentPanelMode::from_sort(sort), true),
            (None, None) => (configured_mode(config), false),
        };
        config.agent_panel_sort = agent_mode.upstream_sort();
        let collapsed_machines = saved
            .collapsed_machines
            .iter()
            .filter_map(|key| parse_endpoint(key))
            .collect();
        let state = Self {
            collapsed: by_endpoint(saved.collapsed_folders),
            collapsed_agent_workspaces: by_endpoint(saved.collapsed_agent_workspaces),
            agent_mode,
            agent_mode_manual: agent_mode_manual && fork_mode.is_some(),
            upstream_saved_sort: upstream.agent_panel_sort,
            prefs_path,
            ..Self::default()
        };
        (state, collapsed_machines)
    }
}

impl ClientShellState {
    /// Saves the fork's client state. Runs whenever upstream saves its own.
    pub(in crate::client::shell) fn persist_folder_preferences(
        &mut self,
        outcome: &mut ClientShellInput,
    ) {
        let Some(path) = self.folders.prefs_path.clone() else {
            return;
        };
        let mut collapsed_machines = self
            .collapsed_endpoints
            .iter()
            .map(ClientEndpointId::storage_key)
            .collect::<Vec<_>>();
        collapsed_machines.sort();
        let state = ForkClientState {
            agent_panel_mode: self
                .folders
                .agent_mode_manual
                .then(|| self.folders.agent_mode.label().to_owned()),
            collapsed_folders: by_key(&self.folders.collapsed),
            collapsed_machines,
            collapsed_agent_workspaces: by_key(&self.folders.collapsed_agent_workspaces),
        };
        if let Err(error) = preferences::store(&path, &state) {
            self.set_endpoint_error(error);
            outcome.repaint = true;
        }
    }

    /// The sort written to upstream's preferences: upstream's own saved
    /// choice, untouched by choices made in the fork.
    pub(in crate::client::shell) fn upstream_saved_agent_panel_sort(
        &self,
    ) -> Option<crate::config::AgentPanelSortConfig> {
        self.folders.upstream_saved_sort
    }

    /// Header click on the sort label: grouped → priority → folders.
    pub(in crate::client::shell) fn cycle_agent_panel_mode(
        &mut self,
        outcome: &mut ClientShellInput,
    ) {
        self.folders.agent_mode = self.folders.agent_mode.next();
        self.folders.agent_mode_manual = true;
        self.agent_panel_sort_manual = true;
        self.config.agent_panel_sort = self.folders.agent_mode.upstream_sort();
        self.agent_scroll = 0;
        self.persist_chrome_preferences(outcome);
        outcome.repaint = true;
    }

    /// After a config reload: a manual choice still wins, otherwise the
    /// configured mode applies.
    pub(in crate::client::shell) fn sync_agent_panel_mode_after_reload(&mut self) {
        if !self.folders.agent_mode_manual && self.folders.upstream_saved_sort.is_none() {
            self.folders.agent_mode = configured_mode(&self.config);
        }
        self.config.agent_panel_sort = self.folders.agent_mode.upstream_sort();
    }

    /// Machine collapse changed; save it with the fork's client state.
    pub(in crate::client::shell) fn machine_collapse_changed(&mut self) {
        let mut outcome = ClientShellInput::default();
        self.persist_folder_preferences(&mut outcome);
    }

    /// A machine's snapshot arrived; forget agents-panel collapse of closed
    /// workspaces.
    pub(in crate::client::shell) fn prune_folder_state_for_snapshot(
        &mut self,
        endpoint_id: &ClientEndpointId,
        snapshot: &ClientShellSnapshot,
    ) {
        if self.folders.prune_closed_workspaces(endpoint_id, snapshot) {
            let mut outcome = ClientShellInput::default();
            self.persist_folder_preferences(&mut outcome);
        }
    }
}
