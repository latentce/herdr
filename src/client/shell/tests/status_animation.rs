use super::*;

const SPINNER_FRAMES: [&str; 8] = ["⠒", "⠲", "⠰", "⠴", "⠤", "⠦", "⠆", "⠖"];

fn agent(pane_id: &str, status: AgentStatus) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: pane_id.into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: None,
        display_agent: None,
        agent: Some("pi".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: status,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: false,
    }
}

fn snapshot_with(
    workspace_status: AgentStatus,
    agents: &[(&str, AgentStatus)],
) -> ClientShellSnapshot {
    let mut projected = snapshot();
    projected.workspaces[0].agent_status = workspace_status;
    for (pane_id, _) in agents.iter().skip(1) {
        let mut pane = projected.panes[0].clone();
        pane.pane_id = (*pane_id).into();
        pane.focused = false;
        projected.panes.push(pane);
    }
    projected.agents = agents
        .iter()
        .map(|(pane_id, status)| agent(pane_id, *status))
        .collect();
    projected
}

fn shell_with(
    style: Option<crate::config::StatusIndicatorStyleConfig>,
    projected: ClientShellSnapshot,
) -> ClientShellState {
    let mut config = Config::default();
    config.ui.status_indicator_style = style;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state
}

fn frame_text(frame: &FrameData) -> String {
    frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect()
}

fn spinner_glyphs(frame: &FrameData) -> Vec<&str> {
    frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .filter(|symbol| SPINNER_FRAMES.contains(symbol))
        .collect()
}

fn assert_no_markers(frame: &FrameData) {
    let text = frame_text(frame);
    assert!(
        !text
            .chars()
            .any(|c| ('\u{100000}'..='\u{10FFFD}').contains(&c)),
        "animation marker reached the screen: {text:?}"
    );
}

/// Whether the shell has an animation frame scheduled.
fn animating(state: &ClientShellState) -> bool {
    state
        .status_animation
        .next_frame_at(std::time::Instant::now())
        .is_some()
}

#[test]
fn animated_spinners_on_screen_share_one_frame_and_schedule_the_next() {
    let mut state = shell_with(
        Some(crate::config::StatusIndicatorStyleConfig::Animated),
        snapshot_with(
            AgentStatus::Working,
            &[
                ("pane_1", AgentStatus::Working),
                ("pane_2", AgentStatus::Working),
            ],
        ),
    );

    let frame = state.compose(106, 30).expect("animated shell");

    assert_no_markers(&frame);
    // The workspace row and both agent rows spin, all on the same frame.
    let spinners = spinner_glyphs(&frame);
    assert_eq!(spinners.len(), 3, "{spinners:?}");
    assert!(spinners.iter().all(|glyph| *glyph == spinners[0]));
    let position = cell_symbol_position(&frame, Rect::new(0, 0, 106, 30), spinners[0]);
    let buffer = frame.to_ratatui_buffer().expect("buffer");
    assert_eq!(buffer[position].fg, state.config.palette.yellow);

    let now = std::time::Instant::now();
    let next = state
        .status_animation
        .next_frame_at(now)
        .expect("spinner scheduled");
    assert!(next <= now + std::time::Duration::from_millis(100));
    // The client loop wakes for the next frame instead of its idle poll.
    assert_eq!(
        state.timer_delay(now),
        next.saturating_duration_since(now)
            .min(std::time::Duration::from_millis(100))
    );
    assert!(state.tick_status_animation(now + std::time::Duration::from_millis(100)));

    let next = state.compose(106, 30).expect("next animated shell");
    assert_no_markers(&next);
}

#[test]
fn animated_blocked_and_done_blink_on_the_half_second() {
    let mut state = shell_with(
        Some(crate::config::StatusIndicatorStyleConfig::Animated),
        snapshot_with(
            AgentStatus::Blocked,
            &[
                ("pane_1", AgentStatus::Blocked),
                ("pane_2", AgentStatus::Done),
            ],
        ),
    );

    let frame = state.compose(106, 30).expect("blinking shell");

    assert_no_markers(&frame);
    assert!(spinner_glyphs(&frame).is_empty());
    assert!(frame_text(&frame).contains('●'));
    let now = std::time::Instant::now();
    let next = state
        .status_animation
        .next_frame_at(now)
        .expect("blink scheduled");
    assert!(next <= now + std::time::Duration::from_millis(500));
    assert!(state.tick_status_animation(now + std::time::Duration::from_millis(500)));
}

#[test]
fn animated_idle_screen_adds_no_timer_or_repaint() {
    let mut state = shell_with(
        Some(crate::config::StatusIndicatorStyleConfig::Animated),
        snapshot_with(
            AgentStatus::Idle,
            &[
                ("pane_1", AgentStatus::Idle),
                ("pane_2", AgentStatus::Unknown),
            ],
        ),
    );

    let frame = state.compose(106, 30).expect("idle shell");

    assert_no_markers(&frame);
    let text = frame_text(&frame);
    assert!(text.contains('○') && text.contains('·'));
    assert!(!animating(&state));
    let later = std::time::Instant::now() + std::time::Duration::from_secs(10);
    assert!(!state.tick_status_animation(later));
}

#[test]
fn dots_and_symbols_never_animate() {
    for style in [
        None,
        Some(crate::config::StatusIndicatorStyleConfig::Symbols),
    ] {
        let mut state = shell_with(
            style,
            snapshot_with(AgentStatus::Working, &[("pane_1", AgentStatus::Working)]),
        );

        let frame = state.compose(106, 30).expect("static shell");

        assert!(spinner_glyphs(&frame).is_empty());
        assert!(!animating(&state));
        let later = std::time::Instant::now() + std::time::Duration::from_secs(10);
        assert!(!state.tick_status_animation(later));
    }
}

#[test]
fn switching_back_to_dots_stops_the_animation() {
    let mut state = shell_with(
        Some(crate::config::StatusIndicatorStyleConfig::Animated),
        snapshot_with(AgentStatus::Working, &[("pane_1", AgentStatus::Working)]),
    );
    state.compose(106, 30).expect("animated shell");
    assert!(animating(&state));

    state.config.status_indicators = crate::config::StatusIndicatorStyle::Dots;
    let frame = state.compose(106, 30).expect("dots shell");

    assert!(spinner_glyphs(&frame).is_empty());
    assert!(!animating(&state));
}

#[test]
fn mobile_header_and_switcher_animate() {
    let mut state = shell_with(
        Some(crate::config::StatusIndicatorStyleConfig::Animated),
        snapshot_with(AgentStatus::Working, &[("pane_1", AgentStatus::Working)]),
    );

    let header = state.compose(44, 30).expect("mobile shell");
    assert_no_markers(&header);
    // The header icon and the "1 working" summary spin together.
    let spinners = spinner_glyphs(&header);
    assert_eq!(spinners.len(), 2, "{spinners:?}");
    assert_eq!(spinners[0], spinners[1]);
    assert!(animating(&state));

    state.mode = ClientShellMode::Navigate;
    let switcher = state.compose(44, 30).expect("mobile switcher");
    assert_no_markers(&switcher);
    // The agents and spaces rows of the switcher.
    let spinners = spinner_glyphs(&switcher);
    assert_eq!(spinners.len(), 2, "{spinners:?}");
    assert_eq!(spinners[0], spinners[1]);
}

#[test]
fn mobile_summary_keeps_idle_unmarked_like_dots() {
    let mut state = shell_with(
        Some(crate::config::StatusIndicatorStyleConfig::Animated),
        snapshot_with(
            AgentStatus::Blocked,
            &[
                ("pane_1", AgentStatus::Blocked),
                ("pane_2", AgentStatus::Idle),
            ],
        ),
    );

    let frame = state.compose(44, 30).expect("mobile shell");

    assert_no_markers(&frame);
    let text = frame_text(&frame);
    assert!(text.contains("● 1 blocked"), "{text}");
    assert!(
        text.contains(" 1 idle") && !text.contains("○ 1 idle"),
        "{text}"
    );
}

#[test]
fn unavailable_surface_still_resolves_sidebar_icons() {
    let mut config = Config::default();
    config.ui.status_indicator_style = Some(crate::config::StatusIndicatorStyleConfig::Animated);
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot_with(
        AgentStatus::Working,
        &[("pane_1", AgentStatus::Working)],
    )));

    let frame = state.compose(106, 30).expect("unavailable shell");

    assert_no_markers(&frame);
}

/// Points the config path at a scratch file and puts both back when dropped,
/// even when an assertion fails.
struct ScratchConfig {
    path: std::path::PathBuf,
    previous: Option<std::ffi::OsString>,
}

impl ScratchConfig {
    fn new(content: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "herdr-animated-indicators-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, content).unwrap();
        let previous = std::env::var_os(crate::config::CONFIG_PATH_ENV_VAR);
        std::env::set_var(crate::config::CONFIG_PATH_ENV_VAR, &path);
        Self { path, previous }
    }

    fn read(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap()
    }
}

impl Drop for ScratchConfig {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var(crate::config::CONFIG_PATH_ENV_VAR, previous),
            None => std::env::remove_var(crate::config::CONFIG_PATH_ENV_VAR),
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn settings_offer_animated_and_dots_turns_it_off() {
    let _guard = crate::config::test_config_env_lock().lock().unwrap();
    let config = ScratchConfig::new("[ui]\nstatus_indicators = \"symbols\"\n");

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let mut outcome = ClientShellInput::default();
    state.open_settings_overlay();
    state.select_settings_section(ClientSettingsSection::Indicators, &mut outcome);
    state.select_settings_choice(usize::MAX);
    let Some(ClientShellOverlay::Settings(settings)) = state.overlay.as_ref() else {
        panic!("settings overlay");
    };
    assert_eq!(settings.selected, 2, "three indicator choices");
    let frame = state.compose(106, 30).expect("settings overlay");
    assert!(frame_text(&frame).contains("animated dots"));

    state.apply_settings_choice(&mut outcome);
    let animated = config.read();
    assert!(
        state.overlay.is_some(),
        "applying an indicator keeps settings open"
    );
    assert!(
        animated.contains("status_indicators = \"symbols\""),
        "{animated}"
    );
    assert!(
        animated.contains("status_indicator_style = \"animated\""),
        "{animated}"
    );
    assert_eq!(
        state.config.status_indicators,
        crate::config::StatusIndicatorStyle::Animated
    );

    state.overlay = None;
    state.open_settings_overlay();
    state.select_settings_section(ClientSettingsSection::Indicators, &mut outcome);
    let Some(ClientShellOverlay::Settings(settings)) = state.overlay.as_ref() else {
        panic!("settings overlay");
    };
    assert_eq!(settings.selected, 2, "animated is the current choice");
    state.select_settings_choice(0);
    state.apply_settings_choice(&mut outcome);
    let dots = config.read();
    assert!(dots.contains("status_indicators = \"dots\""), "{dots}");
    assert!(!dots.contains("status_indicator_style"), "{dots}");
    assert_eq!(
        state.config.status_indicators,
        crate::config::StatusIndicatorStyle::Dots
    );
}

#[test]
fn machines_sidebar_spaces_spin_for_working_workspaces() {
    use crate::client::endpoint::{ClientEndpointId, ProfileId, SavedSshEndpoint};

    let mut state = shell_with(
        Some(crate::config::StatusIndicatorStyleConfig::Animated),
        snapshot_with(AgentStatus::Working, &[("pane_1", AgentStatus::Working)]),
    );
    let profile = SavedSshEndpoint {
        id: ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
        label: "Build".into(),
        target: "dev@build.example".into(),
        session: "agents".into(),
        enabled: true,
    };
    let remote_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&remote_id, ClientEndpointStatus::Online);
    let mut remote = snapshot_with(AgentStatus::Blocked, &[("pane_1", AgentStatus::Blocked)]);
    remote.boot_id = "remote-boot".into();
    remote.workspaces[0].label = "remote-workspace".into();
    state.set_endpoint_snapshot(&remote_id, Box::new(remote));

    let frame = state.compose(106, 30).expect("machines sidebar");

    assert_no_markers(&frame);
    let rows = frame_rows(&frame);
    let row = |label: &str| {
        rows.iter()
            .find(|row| row.contains(label))
            .unwrap_or_else(|| panic!("{label} row in {rows:#?}"))
            .clone()
    };
    let local = row("client-shell");
    assert!(
        SPINNER_FRAMES.iter().any(|glyph| local.contains(glyph)),
        "{local}"
    );
    // A blocked workspace blinks rather than spins.
    let remote = row("remote-workspace");
    assert!(remote.contains('●'), "{remote}");
    assert!(animating(&state));
}
