use std::{
    env, fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossterm::{
    cursor,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    execute,
    terminal::{self, Clear, ClearType},
};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

const MAX_RECENTS: usize = 5;
const COFFEE_URL: &str = "https://buymeacoffee.com/professorvolodymyr";
const COFFEE_TEST_MODE: bool = false;
const DEFAULT_MARKER: &str = "chevron";
const ACCENT_NAMES: [&str; 7] = ["cyan", "magenta", "yellow", "green", "blue", "red", "white"];
const MARKER_NAMES: [&str; 6] = ["chevron", "sparkle", "diamond", "pulse", "ring", "ascii"];
const HELP_TEXT: &[&str] = &[
    "WTFIS",
    "Where the fuck is your project?",
    "A local-first, inline project finder for shells that should know where you work.",
    "",
    "USAGE",
    "  wtfis                  Open the inline project finder",
    "  wtfis QUERY            Search immediately",
    "  cdd QUERY              Short alias for wtfis",
    "  wtfis --set            Configure roots, depth, colors, and commands",
    "  wtfis --up             Recover a failed cd with a global home search",
    "  wtfis --prev           Return to the previous directory",
    "  wtfis --root           Go to the detected project root",
    "  wtfis --last           Return to the last selected project",
    "  wtfis --where          Print the detected project root",
    "  wtfis --home           Go to your home directory",
    "  wtfis --recent         Open recent projects in the selector",
    "  wtfis --help           Show this guide",
    "",
    "DIRECT COMMANDS",
    "  --prev                 Return to the previous directory; needs shell integration",
    "  --root                 Go to a project root detected from .git or manifest files",
    "  --last                 Return to the most recent project selected by WTFIS",
    "  --where                Print the project root without changing directory",
    "  --home                 Go directly to your home directory",
    "  --recent               Open recent projects in the selector",
    "",
    "SEARCH",
    "  Type a project name, path, or typo, then press Enter to open it.",
    "  Multiple words are required matches and can be typed in any order.",
    "  Matching is local and fuzzy. Unique confident matches open immediately.",
    "  Ambiguous matches stay in the selector so you can choose safely.",
    "  No query opens recent projects without scanning the filesystem.",
    "  Search depth includes every layer up to the configured maximum.",
    "  Paths and project data never leave this machine.",
    "  Semantic search is planned, but is not available yet.",
    "",
    "SLASH COMMANDS",
    "  /                      Browse configured command presets",
    "  /opencode              Enter the project and run opencode",
    "  /add                  Attach a preset or custom command to a project",
    "  /exit                 Leave command mode",
    "  /anything              Run any command after entering the project",
    "",
    "CONTROLS",
    "  Up / Down              Move through projects or commands",
    "  Enter                  Open the selected project or run a command",
    "  Esc                    Cancel, clear, or leave the current overlay",
    "  Mouse click            Select or open an item",
    "  Mouse wheel            Ignored inside the inline UI",
    "  Page Up / Page Down    Scroll this help guide",
    "",
    "CONFIGURATION",
    "  wtfis --set            Reopen the settings screen at any time",
    "  Configure search roots, hidden folders, exact depth, accent, marker,",
    "  coffee reminders, and command presets from the inline settings UI.",
    "  The first run shows an introduction before opening setup.",
    "",
    "SHELL INTEGRATION",
    "  Source shell/wtfis.zsh, shell/wtfis.bash, or shell/wtfis.ps1.",
    "  The wrapper changes the parent shell directory after selection.",
    "  Homebrew installs Unix wrappers; Windows uses the PowerShell wrapper.",
    "  Without the wrapper, WTFIS can print paths but cannot change your shell.",
    "  A failed `cd path` prints `Try: wtfis --up` for global recovery.",
    "",
    "HELP CONTROLS",
    "  Up / Down, Page Up / Page Down, Home / End to scroll",
    "  q, Esc, or Enter to close",
];

#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    roots: Option<Vec<PathBuf>>,
    scan_hidden: Option<bool>,
    exact_depth: Option<usize>,
    recent: Option<Vec<PathBuf>>,
    global_search: Option<bool>,
    accent: Option<String>,
    marker: Option<String>,
    setup_completed: Option<bool>,
    command_count: Option<u32>,
    coffee_enabled: Option<bool>,
    coffee_shown: Option<bool>,
    commands: Option<Vec<CommandPreset>>,
    project_commands: Option<Vec<ProjectCommand>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CommandPreset {
    label: String,
    command: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProjectCommand {
    project: PathBuf,
    command: String,
}

#[derive(Clone)]
enum SlashAction {
    Add,
    Custom,
    Exit,
    Run(String),
}

#[derive(Clone)]
struct SlashItem {
    label: String,
    action: SlashAction,
}

struct Selection {
    path: PathBuf,
    command: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        return show_help();
    }
    if args.first().is_some_and(|arg| arg == "--set") {
        return settings();
    }
    if args.first().is_some_and(|arg| arg == "--home") {
        let Some(home) = dirs::home_dir() else {
            eprintln!("wtfis: home directory is unavailable");
            return Ok(());
        };
        return emit_selection(Selection {
            path: home,
            command: None,
        });
    }
    if args.first().is_some_and(|arg| arg == "--prev") {
        let Some(previous) = env::var_os("WTFIS_PREV_CD") else {
            eprintln!("wtfis: no previous directory is available");
            return Ok(());
        };
        return emit_selection(Selection {
            path: PathBuf::from(previous),
            command: None,
        });
    }
    if args
        .first()
        .is_some_and(|arg| matches!(arg.as_str(), "--root" | "--where" | "--last"))
    {
        let config = load_config();
        let path = match args.first().map(String::as_str) {
            Some("--last") => config
                .recent
                .as_ref()
                .and_then(|recent| recent.iter().find(|path| path.is_dir()).cloned()),
            _ => detected_project_root(&config),
        };
        let Some(path) = path else {
            eprintln!("wtfis: no project directory could be detected");
            return Ok(());
        };
        return emit_selection(Selection {
            path,
            command: None,
        });
    }

    let recover_cd = args.first().is_some_and(|arg| arg == "--up");
    let recent_mode = args.first().is_some_and(|arg| arg == "--recent");
    let mut config = load_config();
    if !recover_cd && !config.setup_completed.unwrap_or(false) {
        if io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal() {
            show_first_run_intro()?;
            run_settings(true)?;
            print_shell_setup_hint();
        } else {
            config.setup_completed = Some(true);
            save_config(&config)?;
        }
        config = load_config();
    }
    let coffee_due = record_command(&mut config)?;
    let query = if recover_cd {
        let Some(failed_path) = env::var_os("WTFIS_LAST_CD") else {
            eprintln!("wtfis: no failed cd path is available");
            return Ok(());
        };
        Path::new(&failed_path)
            .file_name()
            .unwrap_or(failed_path.as_os_str())
            .to_string_lossy()
            .into_owned()
    } else if recent_mode {
        String::new()
    } else {
        args.join(" ")
    };
    let (roots, exact_depth) = if recover_cd {
        (default_global_roots(), None)
    } else {
        search_scope(&config)
    };
    let scan_hidden = if recover_cd {
        false
    } else {
        config.scan_hidden.unwrap_or(false)
    };
    let recent = config.recent.clone().unwrap_or_default();
    if !recover_cd && query.contains('/') {
        if let Some(path) = confident_match(&roots, &query) {
            remember(&mut config, path.clone())?;
            emit_selection_or_easter_egg(Selection {
                command: project_command(&config, &path),
                path,
            })?;
            return Ok(());
        }
    }
    let projects = if query.is_empty() || recover_cd {
        None
    } else {
        let mut found = scan(&roots, scan_hidden, exact_depth);
        if let Ok(current) = env::current_dir() {
            if current.is_dir() && !found.contains(&current) {
                found.push(current);
                found.sort();
            }
        }
        Some(found)
    };

    if let Some(projects) = &projects {
        if let Some(path) = confident_match(projects, &query) {
            remember(&mut config, path.clone())?;
            emit_selection_or_easter_egg(Selection {
                command: project_command(&config, &path),
                path,
            })?;
            return Ok(());
        }
    }

    if coffee_due
        && io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && io::stderr().is_terminal()
    {
        show_coffee_popup()?;
    }

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        if query.is_empty() {
            return Ok(());
        }
        return print_best(projects.as_deref().unwrap_or_default(), &query);
    }

    let accent = accent_color(config.accent.as_deref());
    let marker = config
        .marker
        .as_deref()
        .unwrap_or(DEFAULT_MARKER)
        .to_string();
    let selected = picker(
        &roots,
        scan_hidden,
        exact_depth,
        &recent,
        &query,
        &mut config,
        accent,
        &marker,
        recover_cd,
    )?;
    if let Some(selection) = selected {
        remember(&mut config, selection.path.clone())?;
        emit_selection_or_easter_egg(selection)?;
    }
    Ok(())
}

fn show_first_run_intro() -> Result<(), Box<dyn std::error::Error>> {
    let height = terminal::size()
        .map(|(_, rows)| rows.clamp(8, 14))
        .unwrap_or(10);
    let mut session = UiSession::new(height)?;

    loop {
        session.terminal.draw(|frame| {
            render_first_run_intro(frame);
        })?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' '),
                ..
            }) => break,
            Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Down(_)) => break,
            _ => {}
        }
    }

    session.cleanup()?;
    Ok(())
}

fn render_first_run_intro(frame: &mut ratatui::Frame<'_>) {
    let area = frame.area();
    let accent = Color::Magenta;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .title(" WTFIS ")
        .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = vec![
        Line::from(Span::styled(
            "Where the fuck is your project?",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("WTFIS is a local-first fuzzy project finder."),
        Line::from("Type a name, a typo, or a few words from its path."),
        Line::from("Choose a result and your shell enters the project."),
        Line::from(""),
        Line::from(Span::styled(
            "First, let us configure where your projects live.",
            Style::default().fg(accent),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Enter / Space / click to continue",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray)),
        inner,
    );
}

#[cfg(not(windows))]
fn print_shell_setup_hint() {
    let wrapper = if env::var("SHELL").is_ok_and(|shell| shell.ends_with("bash")) {
        "wtfis.bash"
    } else {
        "wtfis.zsh"
    };
    eprintln!(
        "wtfis: automatic cd is off. Run:\n  source \"$(brew --prefix)/share/wtfis/{wrapper}\""
    );
}

#[cfg(windows)]
fn print_shell_setup_hint() {
    eprintln!(
        "wtfis: automatic cd is off. Run in PowerShell:\n  . \"$(Split-Path (Get-Command wtfis.exe).Source)\\wtfis.ps1\""
    );
}

fn detected_project_root(config: &Config) -> Option<PathBuf> {
    let current = env::current_dir().ok()?;
    if let Some(root) = current.ancestors().find(|path| {
        [
            ".git",
            "Cargo.toml",
            "package.json",
            "pyproject.toml",
            "go.mod",
            "Gemfile",
            "composer.json",
        ]
        .iter()
        .any(|marker| path.join(marker).exists())
    }) {
        return Some(root.to_path_buf());
    }
    config
        .recent
        .as_ref()
        .and_then(|recent| {
            recent
                .iter()
                .filter(|path| current.starts_with(path))
                .max_by_key(|path| path.components().count())
                .cloned()
        })
        .or(Some(current))
}

fn default_roots() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let candidates = [
        "GSpace",
        "Projects",
        "Developer",
        "Code",
        "Workspace",
        "work",
        "Documents",
        "Desktop",
    ];
    let roots: Vec<_> = candidates
        .into_iter()
        .map(|name| home.join(name))
        .filter(|path| path.is_dir())
        .collect();
    if roots.is_empty() { vec![home] } else { roots }
}

fn global_search_enabled(config: &Config) -> bool {
    config.global_search.unwrap_or(config.roots.is_none())
}

fn search_scope(config: &Config) -> (Vec<PathBuf>, Option<usize>) {
    if global_search_enabled(config) {
        (default_roots(), None)
    } else {
        (config.roots.clone().unwrap_or_default(), config.exact_depth)
    }
}

fn default_global_roots() -> Vec<PathBuf> {
    vec![dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))]
}

fn accent_color(name: Option<&str>) -> Color {
    match name.unwrap_or("cyan") {
        "magenta" => Color::Magenta,
        "yellow" => Color::Yellow,
        "green" => Color::Green,
        "blue" => Color::Blue,
        "red" => Color::Red,
        "white" => Color::White,
        _ => Color::Cyan,
    }
}

fn marker_frames(name: &str) -> [&'static str; 4] {
    match name {
        "sparkle" => ["✦ ", "✧ ", "✦ ", "✧ "],
        "diamond" => ["◆ ", "◇ ", "◆ ", "◇ "],
        "pulse" => ["● ", "○ ", "● ", "○ "],
        "ring" => ["◉ ", "◎ ", "◉ ", "◎ "],
        "ascii" => ["- ", "\\ ", "| ", "/ "],
        _ => ["› ", "» ", "› ", "» "],
    }
}

fn default_commands() -> Vec<CommandPreset> {
    [
        ("opencode", "opencode"),
        ("opencode --continue", "opencode --continue"),
        ("claude", "claude"),
        ("Codex", "codex"),
        ("yazi", "yazi"),
    ]
    .into_iter()
    .map(|(label, command)| CommandPreset {
        label: label.to_string(),
        command: command.to_string(),
    })
    .collect()
}

fn configured_commands(config: &Config) -> Vec<CommandPreset> {
    config.commands.clone().unwrap_or_else(default_commands)
}

fn project_command(config: &Config, path: &Path) -> Option<String> {
    config
        .project_commands
        .as_ref()?
        .iter()
        .find(|item| item.project == path)
        .map(|item| item.command.clone())
}

fn set_project_command(
    config: &mut Config,
    path: &Path,
    command: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let commands = config.project_commands.get_or_insert_with(Vec::new);
    commands.retain(|item| item.project != path);
    commands.push(ProjectCommand {
        project: path.to_path_buf(),
        command: command.to_string(),
    });
    save_config(config)
}

fn slash_items(config: &Config, query: &str, adding: bool, custom: bool) -> Vec<SlashItem> {
    if custom {
        return Vec::new();
    }
    let filter = if adding {
        ""
    } else {
        query.strip_prefix('/').unwrap_or(query).trim()
    };
    let mut items = Vec::new();
    if !adding {
        items.push(SlashItem {
            label: "/add".to_string(),
            action: SlashAction::Add,
        });
    } else {
        items.push(SlashItem {
            label: "Custom...".to_string(),
            action: SlashAction::Custom,
        });
    }
    items.extend(
        configured_commands(config)
            .into_iter()
            .map(|preset| SlashItem {
                label: format!("/{}", preset.label),
                action: SlashAction::Run(preset.command),
            }),
    );
    if !adding {
        items.push(SlashItem {
            label: "/exit".to_string(),
            action: SlashAction::Exit,
        });
    }
    if filter.is_empty() {
        return items;
    }
    let filter = filter.to_lowercase();
    items
        .into_iter()
        .filter(|item| item.label.to_lowercase().contains(&filter))
        .collect()
}

fn split_command_input(input: &str) -> (String, Option<String>) {
    let trimmed = input.trim();
    if trimmed.starts_with('/') {
        if trimmed.matches('/').count() > 1 || Path::new(trimmed).is_dir() {
            return (trimmed.to_string(), None);
        }
        return (String::new(), Some(trimmed.to_string()));
    }
    let Some(index) = trimmed.find(" /") else {
        return (trimmed.to_string(), None);
    };
    let project = trimmed[..index].trim();
    let command = trimmed[index + 1..].trim();
    if project.is_empty() || !command.starts_with('/') {
        return (trimmed.to_string(), None);
    }
    (project.to_string(), Some(command.to_string()))
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("wtfis/config.toml"))
}

fn load_config() -> Config {
    config_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn scan(roots: &[PathBuf], scan_hidden: bool, exact_depth: Option<usize>) -> Vec<PathBuf> {
    if let Some(depth) = exact_depth {
        return scan_exact_depth(roots, scan_hidden, depth);
    }

    let mut paths = Vec::new();
    for root in roots {
        let walker = WalkDir::new(root)
            .follow_links(false)
            .max_depth(exact_depth.unwrap_or(usize::MAX))
            .into_iter();
        for entry in walker
            .filter_entry(|entry| !ignored_directory(entry.path(), scan_hidden))
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_dir()
                || entry.depth() == 0
                || exact_depth.is_some_and(|depth| entry.depth() != depth)
            {
                continue;
            }
            let path = entry.path();
            if !ignored_directory(path, scan_hidden) {
                paths.push(path.to_path_buf());
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn scan_exact_depth(roots: &[PathBuf], scan_hidden: bool, depth: usize) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for root in roots {
        let walker = WalkDir::new(root)
            .follow_links(false)
            .max_depth(depth)
            .into_iter();
        for entry in walker
            .filter_entry(|entry| !ignored_directory(entry.path(), scan_hidden))
            .filter_map(Result::ok)
        {
            if entry.file_type().is_dir()
                && entry.depth() > 0
                && entry.depth() <= depth
                && !ignored_directory(entry.path(), scan_hidden)
            {
                paths.push(entry.path().to_path_buf());
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn ignored_directory(path: &Path, scan_hidden: bool) -> bool {
    if !scan_hidden
        && path
            .components()
            .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
    {
        return true;
    }
    let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else {
        return false;
    };
    if matches!(
        name.as_ref(),
        "node_modules" | "target" | "build" | "dist" | "vendor" | ".git"
    ) {
        return true;
    }
    let is_home_child = dirs::home_dir()
        .as_deref()
        .is_some_and(|home| path.parent() == Some(home));
    is_home_child
        && matches!(
            name.as_ref(),
            "Library" | "Applications" | "Movies" | "Music" | "Pictures" | ".Trash"
        )
}

fn rank(paths: &[PathBuf], query: &str) -> Vec<(PathBuf, i64)> {
    let query = query.to_lowercase();
    let mut results: Vec<_> = paths
        .iter()
        .filter_map(|path| fuzzy_score(path, &query).map(|score| (path.clone(), score)))
        .collect();
    results.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.components().count().cmp(&b.0.components().count()))
            .then_with(|| a.0.cmp(&b.0))
    });
    results
}

fn fuzzy_score(path: &Path, query: &str) -> Option<i64> {
    let tokens: Vec<_> = query.split_whitespace().collect();
    if tokens.len() > 1 {
        return tokens.into_iter().try_fold(0i64, |total, token| {
            fuzzy_score_term(path, token).map(|score| total + score)
        });
    }
    fuzzy_score_term(path, query)
}

fn fuzzy_score_term(path: &Path, query: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let text = path.to_string_lossy().to_lowercase();
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let compact_query = compact_search_text(query);
    let compact_text = compact_search_text(&text);
    let compact_name = compact_search_text(&name);
    if name == query {
        return Some(10_000);
    }
    if name.starts_with(query) {
        return Some(8_000 - name.len() as i64);
    }
    if name.contains(query) {
        return Some(6_000 - name.len() as i64);
    }
    if compact_name == compact_query {
        return Some(9_500);
    }
    if compact_name.starts_with(&compact_query) {
        return Some(8_500 - compact_name.len() as i64);
    }
    if compact_name.contains(&compact_query) {
        return Some(6_500 - compact_name.len() as i64);
    }
    if compact_text.contains(&compact_query) {
        return Some(7_000 - compact_text.len() as i64);
    }

    let mut score = 0;
    let mut cursor = 0;
    let chars: Vec<_> = text.chars().collect();
    for wanted in query.chars() {
        let Some(pos) = chars[cursor..].iter().position(|c| *c == wanted) else {
            return None;
        };
        let actual = cursor + pos;
        score += if actual == 0
            || chars[actual - 1].is_whitespace()
            || chars[actual - 1] == '/'
            || chars[actual - 1] == '-'
            || chars[actual - 1] == '_'
        {
            20
        } else {
            5
        };
        cursor = actual + 1;
    }
    Some(score - (text.len() as i64 / 10))
}

fn compact_search_text(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<_> = right.chars().collect();
    let mut previous: Vec<_> = (0..=right.len()).collect();
    for (left_index, left_character) in left.chars().enumerate() {
        let mut current = vec![left_index + 1; right.len() + 1];
        for (right_index, right_character) in right.iter().enumerate() {
            current[right_index + 1] = if left_character == *right_character {
                previous[right_index]
            } else {
                1 + previous[right_index]
                    .min(previous[right_index + 1])
                    .min(current[right_index])
            };
        }
        previous = current;
    }
    previous[right.len()]
}

fn confident_match(paths: &[PathBuf], query: &str) -> Option<PathBuf> {
    let ranked = rank(paths, query);
    let (path, score) = ranked.first()?;
    let compact_query = compact_search_text(query);
    let compact_name = compact_search_text(path.file_name()?.to_string_lossy().as_ref());
    let name_distance = edit_distance(&compact_name, &compact_query);
    let path_like = query.contains('/');
    let strong = (compact_query.len() >= 4 && name_distance <= 1) || (path_like && *score > 0);
    if !strong {
        return None;
    }
    if let Some((_, next_score)) = ranked.get(1) {
        if score - next_score < 25 {
            return None;
        }
    }
    Some(path.clone())
}

fn show_help() -> Result<(), Box<dyn std::error::Error>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        println!("{}", HELP_TEXT.join("\n"));
        return Ok(());
    }

    let height = terminal::size()
        .map(|(_, rows)| rows.clamp(7, 24))
        .unwrap_or(16);
    let lines = help_lines(accent_color(Some("cyan")));
    let visible = height.saturating_sub(2) as usize;
    let mut scroll = 0usize;
    let mut session = UiSession::new(height)?;

    loop {
        session.terminal.draw(|frame| {
            render_help(frame, &lines, scroll, accent_color(Some("cyan")));
        })?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q'),
                ..
            }) => break,
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => scroll = scroll.saturating_sub(1),
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => scroll = (scroll + 1).min(lines.len().saturating_sub(visible)),
            Event::Key(KeyEvent {
                code: KeyCode::PageUp,
                ..
            }) => scroll = scroll.saturating_sub(visible.max(1)),
            Event::Key(KeyEvent {
                code: KeyCode::PageDown,
                ..
            }) => scroll = (scroll + visible.max(1)).min(lines.len().saturating_sub(visible)),
            Event::Key(KeyEvent {
                code: KeyCode::Home,
                ..
            }) => scroll = 0,
            Event::Key(KeyEvent {
                code: KeyCode::End, ..
            }) => scroll = lines.len().saturating_sub(visible),
            _ => {}
        }
    }

    session.cleanup()?;
    Ok(())
}

fn help_lines(accent: Color) -> Vec<Line<'static>> {
    HELP_TEXT
        .iter()
        .map(|text| {
            let style = if *text == "WTFIS" || is_help_section(text) {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else if text.is_empty() {
                Style::default()
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(Span::styled(*text, style))
        })
        .collect()
}

fn is_help_section(text: &str) -> bool {
    matches!(
        text,
        "USAGE"
            | "SEARCH"
            | "SLASH COMMANDS"
            | "CONTROLS"
            | "CONFIGURATION"
            | "SHELL INTEGRATION"
            | "HELP CONTROLS"
    )
}

fn picker(
    roots: &[PathBuf],
    scan_hidden: bool,
    exact_depth: Option<usize>,
    recent: &[PathBuf],
    initial: &str,
    config: &mut Config,
    accent: Color,
    marker: &str,
    mut recovery: bool,
) -> Result<Option<Selection>, Box<dyn std::error::Error>> {
    let height = terminal::size()
        .map(|(_, rows)| rows.clamp(7, 12))
        .unwrap_or(8);
    let mut session = UiSession::new(height)?;
    let mut query = initial.to_string();
    let (initial_project_query, _) = split_command_input(initial);
    let mut project_query = initial_project_query;
    let mut selected = 0usize;
    let mut project_selected = 0usize;
    let mut paths = None;
    let mut scan_receiver: Option<Receiver<Vec<PathBuf>>> = None;
    let mut scanning = false;
    let mut last_click: Option<(usize, Instant)> = None;
    let mut finding_frame = 0usize;
    let mut command_add = false;
    let mut add_project_mode = false;
    let mut custom_command = false;
    let mut error_overlay: Option<String> = None;

    let result = loop {
        let (typed_project_query, command_query) = split_command_input(&query);
        let command_mode = command_query.is_some();
        let command_view = command_mode && !add_project_mode;
        if !typed_project_query.is_empty() && paths.is_none() && scan_receiver.is_none() {
            scan_receiver = Some(start_scan(roots.to_vec(), scan_hidden, exact_depth));
            scanning = true;
        }
        if let Some(receiver) = &scan_receiver {
            if let Ok(found_paths) = receiver.try_recv() {
                paths = Some(found_paths);
                scan_receiver = None;
                scanning = false;
            }
        }

        let recent_results: Vec<_> = recent
            .iter()
            .take(MAX_RECENTS)
            .cloned()
            .map(|path| (path, 0))
            .collect();
        if !command_mode {
            project_query = typed_project_query;
        }
        let project_results = if project_query.is_empty() {
            recent_results
        } else {
            rank(paths.as_deref().unwrap_or_default(), &project_query)
        };
        let command_items = if command_view {
            slash_items(
                config,
                command_query.as_deref().unwrap_or("/"),
                command_add,
                custom_command,
            )
        } else {
            Vec::new()
        };
        let item_count = if command_view {
            command_items.len()
        } else {
            project_results.len()
        };
        project_selected = project_selected.min(project_results.len().saturating_sub(1));
        if command_view {
            selected = selected.min(item_count.saturating_sub(1));
        } else {
            selected = project_selected;
        }
        finding_frame = finding_frame.wrapping_add(1);

        if recovery
            && paths.is_some()
            && !scanning
            && !query.is_empty()
            && project_results.is_empty()
        {
            error_overlay = Some("Can't fucking find it 🤯".to_string());
        }

        let mut results_area = Rect::default();
        if let Some(message) = &error_overlay {
            session.terminal.draw(|frame| {
                render_error_screen(frame, message, accent);
            })?;
        } else {
            session.terminal.draw(|frame| {
                results_area = render_frame(
                    frame,
                    &query,
                    &project_results,
                    selected,
                    project_selected,
                    scanning,
                    finding_frame,
                    accent,
                    marker,
                    command_view.then_some(command_items.as_slice()),
                    command_add,
                    command_query.as_deref(),
                );
            })?;
        }

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let input_event = event::read()?;
        if error_overlay.is_some() {
            match input_event {
                Event::Key(KeyEvent {
                    code: KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' '),
                    ..
                }) => {
                    error_overlay = None;
                    recovery = false;
                    query.clear();
                    project_query.clear();
                    command_add = false;
                    add_project_mode = false;
                    custom_command = false;
                    selected = 0;
                    project_selected = 0;
                }
                Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Down(_)) => {
                    error_overlay = None;
                    recovery = false;
                    query.clear();
                    project_query.clear();
                    command_add = false;
                    add_project_mode = false;
                    custom_command = false;
                    selected = 0;
                    project_selected = 0;
                }
                _ => {}
            }
            continue;
        }
        if command_view {
            match input_event {
                Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                }) => {
                    if command_add {
                        if add_project_mode {
                            add_project_mode = false;
                            command_add = false;
                            query = "/".to_string();
                        } else if custom_command {
                            custom_command = false;
                            query = "/add ".to_string();
                        } else {
                            command_add = false;
                            query = "/".to_string();
                        }
                        selected = 0;
                    } else {
                        break None;
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => {
                    if add_project_mode {
                        add_project_mode = false;
                        query = "/add ".to_string();
                        selected = 0;
                        continue;
                    }
                    if custom_command {
                        let command = command_query
                            .as_deref()
                            .unwrap_or_default()
                            .strip_prefix("/add custom")
                            .unwrap_or_default()
                            .trim();
                        if command.is_empty() {
                            error_overlay =
                                Some("type a custom command before pressing Enter".to_string());
                            continue;
                        }
                        let Some((path, _)) = project_results.get(project_selected) else {
                            error_overlay =
                                Some("Select a project before adding a command".to_string());
                            continue;
                        };
                        if let Err(error) = set_project_command(config, path, command) {
                            error_overlay = Some(format!("could not save command: {error}"));
                        } else {
                            command_add = false;
                            custom_command = false;
                            query.clear();
                            project_query.clear();
                            selected = 0;
                        }
                        continue;
                    }
                    if !command_add && command_items.is_empty() {
                        let command = command_query
                            .as_deref()
                            .unwrap_or_default()
                            .strip_prefix('/')
                            .unwrap_or_default()
                            .trim();
                        if !command.is_empty() {
                            let Some((path, _)) = project_results.get(project_selected) else {
                                error_overlay =
                                    Some("Select a project before running a command".to_string());
                                continue;
                            };
                            break Some(Selection {
                                path: path.clone(),
                                command: Some(command.to_string()),
                            });
                        }
                    }
                    let Some(item) = command_items.get(selected).cloned() else {
                        continue;
                    };
                    match item.action {
                        SlashAction::Exit => break None,
                        SlashAction::Add => {
                            if project_results.is_empty() {
                                error_overlay =
                                    Some("Select a project before adding a command".to_string());
                            } else {
                                command_add = true;
                                add_project_mode = true;
                                query = "/add ".to_string();
                                selected = project_selected;
                            }
                        }
                        SlashAction::Custom => {
                            custom_command = true;
                            query = "/add custom ".to_string();
                            selected = 0;
                        }
                        SlashAction::Run(command) => {
                            let Some((path, _)) = project_results.get(project_selected) else {
                                error_overlay =
                                    Some("Select a project before running a command".to_string());
                                continue;
                            };
                            if command_add {
                                if let Err(error) = set_project_command(config, path, &command) {
                                    error_overlay =
                                        Some(format!("could not save command: {error}"));
                                } else {
                                    command_add = false;
                                    custom_command = false;
                                    query.clear();
                                    project_query.clear();
                                    selected = 0;
                                }
                            } else {
                                break Some(Selection {
                                    path: path.clone(),
                                    command: Some(command),
                                });
                            }
                        }
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Up, ..
                }) => selected = selected.saturating_sub(1),
                Event::Key(KeyEvent {
                    code: KeyCode::Down,
                    ..
                }) => selected = (selected + 1).min(command_items.len().saturating_sub(1)),
                Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    ..
                }) => {
                    query.pop();
                    if query == "/" {
                        selected = 0;
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char(c),
                    ..
                }) => query.push(c),
                Event::Mouse(mouse)
                    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) =>
                {
                    let visible = results_area.height as usize;
                    let start = result_start(command_items.len(), selected, visible);
                    if results_area.contains(Position::new(mouse.column, mouse.row)) {
                        selected = start + mouse.row.saturating_sub(results_area.y) as usize;
                    }
                }
                _ => {}
            }
            continue;
        }
        match input_event {
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => {
                if add_project_mode {
                    add_project_mode = false;
                    command_add = false;
                    query = "/".to_string();
                    selected = 0;
                } else {
                    break None;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                if add_project_mode {
                    add_project_mode = false;
                    query = "/add ".to_string();
                    selected = 0;
                    continue;
                }
                if let Some((path, _)) = project_results.get(project_selected) {
                    break Some(Selection {
                        path: path.clone(),
                        command: project_command(config, path),
                    });
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                project_selected = project_selected.saturating_sub(1);
                selected = project_selected;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => {
                project_selected =
                    (project_selected + 1).min(project_results.len().saturating_sub(1));
                selected = project_selected;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                ..
            }) => {
                query.pop();
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => break None,
            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                ..
            }) => query.push(c),
            Event::Mouse(mouse)
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) =>
            {
                let visible_count = results_area.height as usize;
                let start = result_start(project_results.len(), project_selected, visible_count);
                if results_area.contains(Position::new(mouse.column, mouse.row)) {
                    let clicked = start + mouse.row.saturating_sub(results_area.y) as usize;
                    if clicked >= project_results.len() {
                        continue;
                    }
                    if last_click.is_some_and(|(previous, time)| {
                        previous == clicked && time.elapsed() < Duration::from_millis(500)
                    }) {
                        if let Some((path, _)) = project_results.get(clicked) {
                            break Some(Selection {
                                path: path.clone(),
                                command: project_command(config, path),
                            });
                        }
                    }
                    project_selected = clicked;
                    selected = project_selected;
                    last_click = Some((clicked, Instant::now()));
                }
            }
            _ => {}
        }
    };

    session.cleanup()?;
    Ok(result)
}

struct UiSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    height: u16,
    mouse_capture: bool,
    raw_mode: bool,
    cleaned: bool,
}

impl UiSession {
    fn new(height: u16) -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut backend = CrosstermBackend::new(io::stdout());
        if let Err(error) = execute!(backend, cursor::SavePosition) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }

        let terminal = match Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        ) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = execute!(io::stdout(), cursor::RestorePosition);
                let _ = terminal::disable_raw_mode();
                return Err(error);
            }
        };
        let session = Self {
            terminal,
            height,
            mouse_capture: false,
            raw_mode: true,
            cleaned: false,
        };
        let mut session = session;
        if let Err(error) = execute!(session.terminal.backend_mut(), EnableMouseCapture) {
            let _ = session.cleanup();
            return Err(error);
        }
        session.mouse_capture = true;
        Ok(session)
    }

    fn cleanup(&mut self) -> io::Result<()> {
        if self.cleaned {
            return Ok(());
        }
        let mut first_error = None;
        if self.mouse_capture {
            if let Err(error) = execute!(self.terminal.backend_mut(), DisableMouseCapture) {
                first_error = Some(error);
            }
            self.mouse_capture = false;
        }
        if let Err(error) = self.terminal.clear() {
            first_error.get_or_insert(error);
        }
        if let Err(error) = execute!(
            self.terminal.backend_mut(),
            cursor::RestorePosition,
            crossterm::style::Print(format!("\x1b[{}M", self.height)),
            Clear(ClearType::CurrentLine),
            cursor::MoveToColumn(0),
        ) {
            first_error.get_or_insert(error);
        }
        if let Err(error) = self.terminal.show_cursor() {
            first_error.get_or_insert(error);
        }
        if self.raw_mode {
            if let Err(error) = terminal::disable_raw_mode() {
                first_error.get_or_insert(error);
            }
            self.raw_mode = false;
        }
        self.cleaned = true;
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for UiSession {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn start_scan(
    roots: Vec<PathBuf>,
    scan_hidden: bool,
    exact_depth: Option<usize>,
) -> Receiver<Vec<PathBuf>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(scan(&roots, scan_hidden, exact_depth));
    });
    receiver
}

fn result_start(total: usize, selected: usize, visible: usize) -> usize {
    if visible == 0 || total <= visible {
        0
    } else {
        selected.saturating_sub(visible - 1).min(total - visible)
    }
}

fn render_help(
    frame: &mut ratatui::Frame<'_>,
    lines: &[Line<'static>],
    scroll: usize,
    accent: Color,
) {
    let area = frame.area();
    let guides = Line::from(vec![
        Span::styled(
            "↑↓",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" scroll  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "q",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" close", Style::default().fg(Color::DarkGray)),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .title(" WTFIS HELP ")
        .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
        .title_bottom(guides);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(lines.to_vec())
            .scroll((scroll.min(lines.len()) as u16, 0))
            .style(Style::default().fg(Color::Gray)),
        inner,
    );
}

fn render_error_screen(frame: &mut ratatui::Frame<'_>, message: &str, accent: Color) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Red).fg(Color::White)),
        area,
    );
    let content = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(4) / 2,
        area.width,
        area.height.min(4),
    );
    let lines = vec![
        Line::from(Span::styled(
            message,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Esc  Space  Enter  to continue",
            Style::default().fg(accent),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().bg(Color::Red).fg(Color::White)),
        content,
    );
}

fn render_frame(
    frame: &mut ratatui::Frame<'_>,
    query: &str,
    results: &[(PathBuf, i64)],
    selected: usize,
    project_selected: usize,
    scanning: bool,
    finding_frame: usize,
    accent: Color,
    marker: &str,
    command_items: Option<&[SlashItem]>,
    command_add: bool,
    command_query: Option<&str>,
) -> Rect {
    let area = frame.area();
    let guide_icon = Style::default().fg(accent).add_modifier(Modifier::BOLD);
    let guide_text = Style::default().fg(Color::DarkGray);
    let mut guide_spans = vec![
        Span::raw(" "),
        Span::styled("↑↓", guide_icon),
        Span::styled(" move  ", guide_text),
        Span::styled("↵", guide_icon),
        Span::styled(" open  ", guide_text),
        Span::styled("⎋", guide_icon),
        Span::styled(" cancel  ", guide_text),
        Span::styled("/", guide_icon),
        Span::styled(" commands  ", guide_text),
    ];
    if command_items.is_some() || command_add {
        if let Some((path, _)) = results.get(project_selected) {
            guide_spans.push(Span::styled("  target: ", guide_icon));
            guide_spans.push(Span::styled(
                path.file_name().unwrap_or_default().to_string_lossy(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
    let guides = Line::from(guide_spans);
    let marker_sequence = marker_frames(marker);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .title(" WTFIS ")
        .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
        .title_bottom(guides);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    let input_area = sections[0];
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent));
    let input_inner = input_block.inner(input_area);
    frame.render_widget(input_block, input_area);
    let input = if query.is_empty() {
        Line::from(Span::styled(
            "Fucking Find Something...",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Line::from(Span::styled(
            query,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let cursor_width = if query.is_empty() { 0 } else { input.width() };
    let cursor_x = input_inner
        .x
        .saturating_add(cursor_width.min(u16::MAX as usize) as u16)
        .min(input_inner.right().saturating_sub(1));
    frame.render_widget(Paragraph::new(input), input_inner);
    frame.set_cursor_position(Position::new(cursor_x, input_inner.y));

    if let Some(command_items) = command_items {
        let visible = sections[1].height as usize;
        let start = result_start(command_items.len(), selected, visible);
        let end = (start + visible).min(command_items.len());
        if start == end {
            let message = if command_query.is_some_and(|value| value.starts_with("/add custom")) {
                "Type a custom command, then press Enter"
            } else if command_add {
                "Choose a command to add"
            } else {
                "No command matches"
            };
            frame.render_widget(
                Paragraph::new(message).style(Style::default().fg(Color::DarkGray)),
                sections[1],
            );
        } else {
            let frames = marker_sequence;
            let items: Vec<ListItem> = command_items[start..end]
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let actual = start + index;
                    let selected_style = actual == selected;
                    let marker_style = if selected_style {
                        Style::default().fg(accent).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    let label_style = if selected_style {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            if selected_style {
                                frames[finding_frame % frames.len()]
                            } else {
                                "  "
                            },
                            marker_style,
                        ),
                        Span::styled(item.label.as_str(), label_style),
                    ]))
                })
                .collect();
            frame.render_widget(List::new(items), sections[1]);
        }
        return sections[1];
    }

    let visible = sections[1].height as usize;
    let start = result_start(results.len(), project_selected, visible);
    let end = (start + visible).min(results.len());
    if start == end {
        let message = if scanning {
            let frames = ["[    ]", "[.   ]", "[..  ]", "[...]", "[ .. ]"];
            Line::from(vec![
                Span::styled(
                    frames[finding_frame % frames.len()],
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Finding your project...", Style::default().fg(Color::Gray)),
            ])
        } else if query.is_empty() {
            Line::from(Span::styled(
                "Your recent projects will appear here",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(Span::styled(
                "No matching folders",
                Style::default().fg(Color::DarkGray),
            ))
        };
        frame.render_widget(Paragraph::new(message), sections[1]);
    } else {
        let items: Vec<ListItem> = results[start..end]
            .iter()
            .enumerate()
            .map(|(index, (path, _))| {
                let actual = start + index;
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let selected_style = actual == project_selected;
                let marker = if selected_style {
                    marker_sequence[finding_frame % marker_sequence.len()]
                } else {
                    "  "
                };
                let marker_style = if selected_style {
                    Style::default().fg(accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let name_style = if selected_style {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                let path_style = Style::default().fg(Color::DarkGray);
                ListItem::new(Line::from(vec![
                    Span::styled(marker, marker_style),
                    Span::styled(name, name_style),
                    Span::styled("  ", path_style),
                    Span::styled(path.display().to_string(), path_style),
                ]))
            })
            .collect();
        frame.render_widget(List::new(items), sections[1]);
    }
    sections[1]
}

fn remember(config: &mut Config, path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let recent = config.recent.get_or_insert_with(Vec::new);
    recent.retain(|item| item != &path);
    recent.insert(0, path);
    recent.truncate(MAX_RECENTS);
    save_config(config)
}

fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path().ok_or("cannot determine config directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}

fn record_command(config: &mut Config) -> Result<bool, Box<dyn std::error::Error>> {
    let count = config.command_count.unwrap_or(0).saturating_add(1);
    config.command_count = Some(count);
    let due = config.coffee_enabled.unwrap_or(true)
        && (COFFEE_TEST_MODE || (count >= 15 && !config.coffee_shown.unwrap_or(false)));
    if due && !COFFEE_TEST_MODE {
        config.coffee_shown = Some(true);
    }
    save_config(config)?;
    Ok(due)
}

const ALREADY_HERE_MESSAGES: [&str; 5] = [
    "╔════════════════════════════════╗\n║  WTFIS? YOU'RE FUCKING HERE.   ║\n║                                ║\n║  cd: unnecessary               ║\n║  brain: temporarily offline   ║\n╚════════════════════════════════╝",
    "┌──────────────────────────────┐\n│  ACHIEVEMENT UNLOCKED        │\n│                              │\n│  `cd` into the folder        │\n│  you're already standing in. │\n│                              │\n│  Absolute walnut behavior.   │\n└──────────────────────────────┘",
    "       YOU ARE HERE\n          |\n       \\  o  /\n        \\ | /\n         / \\\n\n  Congratulations, dumbass.\n  You found your current folder.",
    "╭──────────────────────────────╮\n│  YOU ARE ALREADY HERE         │\n│                              │\n│  `cd` completed successfully │\n│  approximately nowhere.     │\n│                              │\n│  Magnificent work, idiot.   │\n╰──────────────────────────────╯",
    "╭────────────────────────────╮\n│  DESTINATION REACHED       │\n│                            │\n│  You are already here,    │\n│  you magnificent dumbass. │\n│                            │\n│  Recalculating purpose... │\n╰────────────────────────────╯",
];

fn emit_selection_or_easter_egg(selection: Selection) -> Result<(), Box<dyn std::error::Error>> {
    if selection.command.is_none()
        && io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && io::stderr().is_terminal()
        && is_current_directory(&selection.path)
    {
        return show_already_here_popup();
    }
    emit_selection(selection)
}

fn is_current_directory(path: &Path) -> bool {
    let Ok(current) = env::current_dir() else {
        return false;
    };
    path == current
        || fs::canonicalize(path)
            .ok()
            .is_some_and(|resolved| resolved == current)
}

fn show_already_here_popup() -> Result<(), Box<dyn std::error::Error>> {
    let height = terminal::size()
        .map(|(_, rows)| rows.clamp(8, 14))
        .unwrap_or(10);
    let message_index = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize % ALREADY_HERE_MESSAGES.len())
        .unwrap_or(0);
    let message = ALREADY_HERE_MESSAGES[message_index];
    let mut session = UiSession::new(height)?;
    let deadline = Instant::now() + Duration::from_secs(3);

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        session
            .terminal
            .draw(|frame| render_already_here(frame, message))?;
        if !event::poll(remaining.min(Duration::from_millis(100)))? {
            continue;
        }
        if let Event::Key(KeyEvent {
            code: KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q'),
            ..
        }) = event::read()?
        {
            break;
        }
    }
    session.cleanup()?;
    Ok(())
}

fn render_already_here(frame: &mut ratatui::Frame<'_>, message: &str) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Black).fg(Color::White)),
        area,
    );
    let width = message
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(1)
        .saturating_add(4)
        .min(area.width as usize) as u16;
    let height = message
        .lines()
        .count()
        .saturating_add(2)
        .min(area.height as usize) as u16;
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" WTFIS ")
        .title_style(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(message)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
        inner,
    );
}

fn open_url(url: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return Command::new("open").arg(url).status().map(|_| ());
    }
    #[cfg(target_os = "windows")]
    {
        return Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
            .map(|_| ());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Command::new("xdg-open").arg(url).status().map(|_| ())
    }
}

fn show_coffee_popup() -> Result<(), Box<dyn std::error::Error>> {
    let height = terminal::size()
        .map(|(_, rows)| rows.clamp(7, 12))
        .unwrap_or(8);
    let mut session = UiSession::new(height)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let open_link = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break false;
        }
        session
            .terminal
            .draw(|frame| render_coffee_popup(frame, remaining))?;
        if !event::poll(remaining.min(Duration::from_millis(100)))? {
            continue;
        }
        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => break false,
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => break true,
            Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Down(_)) => break true,
            _ => {}
        }
    };
    if open_link {
        let _ = open_url(COFFEE_URL);
    }
    session.cleanup()?;
    Ok(())
}

fn render_coffee_popup(frame: &mut ratatui::Frame<'_>, remaining: Duration) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Yellow).fg(Color::Black)),
        area,
    );
    let width = area.width.saturating_sub(4).min(64);
    let height = area.height.saturating_sub(4).min(9);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let popup_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Black))
        .style(Style::default().bg(Color::Yellow).fg(Color::Black))
        .title(" BUY ME A COFFEE ")
        .title_style(
            Style::default()
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );
    let inner = popup_block.inner(popup);
    frame.render_widget(popup_block, popup);
    let message = vec![
        Line::from(Span::styled(
            "No Hot Coffe?🥲☕️",
            Style::default()
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "[ OKAY ]",
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Enter to open  •  Esc to skip",
            Style::default().fg(Color::Black),
        )),
        Line::from(Span::styled(
            format!("auto-closing in {}s", remaining.as_secs().saturating_add(1)),
            Style::default().fg(Color::Black),
        )),
    ];
    frame.render_widget(
        Paragraph::new(message)
            .alignment(Alignment::Center)
            .style(Style::default().bg(Color::Yellow).fg(Color::Black)),
        inner,
    );
}

fn emit_selection(selection: Selection) -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(output_path) = env::var("WTFIS_OUTPUT") {
        let mut output = format!("{}\n", selection.path.display());
        if let Some(command) = selection.command {
            output.push_str(&command);
            output.push('\n');
        }
        fs::write(output_path, output)?;
    } else {
        println!("{}", selection.path.display());
        if io::stdout().is_terminal() && io::stderr().is_terminal() {
            print_shell_setup_hint();
        }
    }
    Ok(())
}

fn print_best(paths: &[PathBuf], query: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some((path, _)) = rank(paths, query).first() {
        println!("{}", path.display());
    }
    Ok(())
}

fn settings() -> Result<(), Box<dyn std::error::Error>> {
    run_settings(false)
}

fn run_settings(wizard: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        eprintln!("wtfis: settings requires an interactive terminal");
        return Ok(());
    }

    let mut state = SettingsState {
        config: load_config(),
        selected: 0,
        adding: false,
        adding_command: false,
        path_input: String::new(),
        error: None,
    };
    let height = terminal::size()
        .map(|(_, rows)| rows.clamp(10, 18))
        .unwrap_or(12);
    let mut session = UiSession::new(height)?;
    let mut rows_area = Rect::default();
    let applied = loop {
        session.terminal.draw(|frame| {
            rows_area = render_settings(frame, &state, wizard);
        })?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }

        let event = event::read()?;
        if state.error.is_some() {
            match event {
                Event::Key(KeyEvent {
                    code: KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' '),
                    ..
                }) => state.error = None,
                Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Down(_)) => {
                    state.error = None;
                }
                _ => {}
            }
            continue;
        }
        if state.adding || state.adding_command {
            match event {
                Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                }) => {
                    state.adding = false;
                    state.adding_command = false;
                    state.path_input.clear();
                    state.error = None;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => {
                    if state.adding_command {
                        add_settings_command(&mut state);
                    } else {
                        add_settings_path(&mut state);
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    ..
                }) => {
                    state.path_input.pop();
                    state.error = None;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char(c),
                    ..
                }) => {
                    state.path_input.push(c);
                    state.error = None;
                }
                _ => {}
            }
            continue;
        }

        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) => break false,
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                state.selected = state.selected.saturating_sub(1);
                state.error = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => {
                state.selected = (state.selected + 1).min(settings_row_count(&state) - 1);
                state.error = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Left,
                ..
            }) => adjust_settings(&mut state, -1),
            Event::Key(KeyEvent {
                code: KeyCode::Right,
                ..
            }) => adjust_settings(&mut state, 1),
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                if state.selected == SETTINGS_ADD_ROW {
                    state.adding = true;
                    state.error = None;
                } else if state.selected == settings_command_add_row(&state) {
                    state.adding_command = true;
                    state.error = None;
                } else if apply_settings(&mut state) {
                    break true;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('a'),
                ..
            }) => {
                state.adding = true;
                state.adding_command = false;
                state.error = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('x'),
                ..
            }) => remove_settings_path(&mut state),
            Event::Mouse(mouse)
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) =>
            {
                let visible = rows_area.height as usize;
                let start = result_start(settings_row_count(&state), state.selected, visible);
                if rows_area.contains(Position::new(mouse.column, mouse.row)) {
                    let clicked = start + mouse.row.saturating_sub(rows_area.y) as usize;
                    if clicked < settings_row_count(&state) {
                        state.selected = clicked;
                        match clicked {
                            SETTINGS_GLOBAL_ROW | SETTINGS_HIDDEN_ROW | SETTINGS_COFFEE_ROW => {
                                adjust_settings(&mut state, 1)
                            }
                            SETTINGS_DEPTH_ROW => adjust_settings(&mut state, 1),
                            SETTINGS_ADD_ROW => {
                                state.adding = true;
                                state.adding_command = false;
                                state.error = None;
                            }
                            _ if clicked == settings_command_add_row(&state) => {
                                state.adding_command = true;
                                state.error = None;
                            }
                            _ if clicked == settings_accent_row(&state) => {
                                adjust_settings(&mut state, 1)
                            }
                            _ if clicked == settings_marker_row(&state) => {
                                adjust_settings(&mut state, 1)
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    };
    session.cleanup()?;
    if applied {
        eprintln!("wtfis: settings applied");
    } else if wizard {
        state.config.setup_completed = Some(true);
        save_config(&state.config)?;
    }
    Ok(())
}

const SETTINGS_GLOBAL_ROW: usize = 0;
const SETTINGS_HIDDEN_ROW: usize = 1;
const SETTINGS_DEPTH_ROW: usize = 2;
const SETTINGS_COFFEE_ROW: usize = 3;
const SETTINGS_ADD_ROW: usize = 4;
const SETTINGS_PATH_START: usize = 5;

struct SettingsState {
    config: Config,
    selected: usize,
    adding: bool,
    adding_command: bool,
    path_input: String,
    error: Option<String>,
}

fn settings_path_count(state: &SettingsState) -> usize {
    state.config.roots.as_ref().map_or(0, Vec::len)
}

fn settings_accent_row(state: &SettingsState) -> usize {
    SETTINGS_PATH_START + settings_path_count(state)
}

fn settings_marker_row(state: &SettingsState) -> usize {
    settings_accent_row(state) + 1
}

fn settings_command_add_row(state: &SettingsState) -> usize {
    settings_marker_row(state) + 1
}

fn settings_command_start(state: &SettingsState) -> usize {
    settings_command_add_row(state) + 1
}

fn settings_command_count(state: &SettingsState) -> usize {
    configured_commands(&state.config).len()
}

fn settings_row_count(state: &SettingsState) -> usize {
    settings_command_start(state) + settings_command_count(state)
}

fn depth_label(depth: Option<usize>) -> &'static str {
    match depth {
        None => "all levels",
        Some(1) => "1 level",
        Some(2) => "2 levels",
        Some(3) => "3 levels",
        Some(_) => "custom",
    }
}

fn marker_label(name: &str) -> &'static str {
    match name {
        "sparkle" => "✦ ✧",
        "diamond" => "◆ ◇",
        "pulse" => "● ○",
        "ring" => "◉ ◎",
        "ascii" => "- \\ | /",
        _ => "› »",
    }
}

fn cycle_index(current: usize, length: usize, delta: i8) -> usize {
    (current as i16 + delta as i16).rem_euclid(length as i16) as usize
}

fn adjust_settings(state: &mut SettingsState, delta: i8) {
    match state.selected {
        SETTINGS_GLOBAL_ROW => {
            state.config.global_search = Some(!global_search_enabled(&state.config));
        }
        SETTINGS_HIDDEN_ROW => {
            let current = state.config.scan_hidden.unwrap_or(false);
            state.config.scan_hidden = Some(!current);
        }
        SETTINGS_COFFEE_ROW => {
            let current = state.config.coffee_enabled.unwrap_or(true);
            state.config.coffee_enabled = Some(!current);
        }
        SETTINGS_DEPTH_ROW => {
            let options = [None, Some(1), Some(2), Some(3)];
            let current = options
                .iter()
                .position(|option| *option == state.config.exact_depth)
                .unwrap_or(0);
            state.config.exact_depth = options[cycle_index(current, options.len(), delta)];
        }
        row if row == settings_accent_row(state) => {
            let current = state.config.accent.as_deref().unwrap_or("cyan");
            let index = ACCENT_NAMES
                .iter()
                .position(|name| *name == current)
                .unwrap_or(0);
            state.config.accent =
                Some(ACCENT_NAMES[cycle_index(index, ACCENT_NAMES.len(), delta)].to_string());
        }
        row if row == settings_marker_row(state) => {
            let current = state.config.marker.as_deref().unwrap_or(DEFAULT_MARKER);
            let index = MARKER_NAMES
                .iter()
                .position(|name| *name == current)
                .unwrap_or(0);
            state.config.marker =
                Some(MARKER_NAMES[cycle_index(index, MARKER_NAMES.len(), delta)].to_string());
        }
        _ => {}
    }
    state.error = None;
}

fn expand_settings_path(input: &str) -> PathBuf {
    if input == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(input));
    }
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(input)
}

fn add_settings_path(state: &mut SettingsState) {
    let input = state.path_input.trim();
    if input.is_empty() {
        state.error = Some("enter a folder path".to_string());
        return;
    }
    let path = expand_settings_path(input);
    if !path.is_dir() {
        state.error = Some(format!("path not found: {}", path.display()));
        return;
    }
    let roots = state.config.roots.get_or_insert_with(Vec::new);
    if roots.iter().any(|root| root == &path) {
        state.error = Some("that folder is already configured".to_string());
        return;
    }
    roots.push(path);
    state.selected = SETTINGS_PATH_START + roots.len() - 1;
    state.adding = false;
    state.path_input.clear();
    state.error = None;
}

fn add_settings_command(state: &mut SettingsState) {
    let input = state.path_input.trim();
    if input.is_empty() {
        state.error = Some("enter a command".to_string());
        return;
    }
    let (label, command) = input
        .split_once('=')
        .map(|(label, command)| (label.trim(), command.trim()))
        .unwrap_or((input, input));
    if label.is_empty() || command.is_empty() {
        state.error = Some("use name=command, for example editor=vim".to_string());
        return;
    }
    let commands = state.config.commands.get_or_insert_with(default_commands);
    if commands
        .iter()
        .any(|item| item.label.eq_ignore_ascii_case(label))
    {
        state.error = Some("that command preset already exists".to_string());
        return;
    }
    commands.push(CommandPreset {
        label: label.to_string(),
        command: command.to_string(),
    });
    let command_count = commands.len();
    state.selected = settings_command_start(state) + command_count - 1;
    state.adding_command = false;
    state.path_input.clear();
    state.error = None;
}

fn remove_settings_path(state: &mut SettingsState) {
    if state.selected >= SETTINGS_PATH_START
        && state.selected < SETTINGS_PATH_START + settings_path_count(state)
    {
        if let Some(roots) = state.config.roots.as_mut() {
            roots.remove(state.selected - SETTINGS_PATH_START);
        }
    } else if state.selected >= settings_command_start(state)
        && state.selected < settings_command_start(state) + settings_command_count(state)
    {
        let index = state.selected - settings_command_start(state);
        state
            .config
            .commands
            .get_or_insert_with(default_commands)
            .remove(index);
    } else {
        return;
    }
    state.selected = state.selected.min(settings_row_count(state) - 1);
    state.error = None;
}

fn apply_settings(state: &mut SettingsState) -> bool {
    if !global_search_enabled(&state.config) {
        let roots = state.config.roots.as_deref().unwrap_or_default();
        if roots.is_empty() {
            state.error = Some("add a search folder or enable global search".to_string());
            return false;
        }
        if let Some(path) = roots.iter().find(|path| !path.is_dir()) {
            state.error = Some(format!("path not found: {}", path.display()));
            return false;
        }
    }
    state.config.setup_completed = Some(true);
    match save_config(&state.config) {
        Ok(()) => true,
        Err(error) => {
            state.error = Some(format!("could not save settings: {error}"));
            false
        }
    }
}

fn render_settings(frame: &mut ratatui::Frame<'_>, state: &SettingsState, wizard: bool) -> Rect {
    let area = frame.area();
    let accent = accent_color(state.config.accent.as_deref());
    if let Some(error) = state.error.as_deref() {
        render_error_screen(frame, error, accent);
        return Rect::default();
    }
    let icon_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(Color::DarkGray);
    let guides = Line::from(vec![
        Span::raw(" "),
        Span::styled("↑↓", icon_style),
        Span::styled(" select  ", text_style),
        Span::styled("←→", icon_style),
        Span::styled(" change  ", text_style),
        Span::styled("↵", icon_style),
        Span::styled(" apply  ", text_style),
        Span::styled("⎋", icon_style),
        Span::styled(" cancel  ", text_style),
        Span::styled("a", icon_style),
        Span::styled(" add  ", text_style),
        Span::styled("x", icon_style),
        Span::styled(" remove", text_style),
    ]);
    let title = if wizard {
        " WTFIS FIRST RUN "
    } else {
        " WTFIS SETTINGS "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .title(title)
        .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
        .title_bottom(guides);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (rows_area, input_area, error_area) = if state.adding || state.adding_command {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(inner);
        (sections[2], Some(sections[0]), Some(sections[1]))
    } else if state.error.is_some() {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner);
        (sections[1], None, Some(sections[0]))
    } else {
        (inner, None, None)
    };

    if let Some(input_area) = input_area {
        let input_title = if state.adding_command {
            " add command "
        } else {
            " add folder "
        };
        let input_placeholder = if state.adding_command {
            "name=command, for example editor=vim"
        } else {
            "Type a folder path..."
        };
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent))
            .title(input_title);
        let input_inner = input_block.inner(input_area);
        frame.render_widget(input_block, input_area);
        let line = if state.path_input.is_empty() {
            Line::from(Span::styled(
                input_placeholder,
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(Span::styled(
                state.path_input.as_str(),
                Style::default().fg(Color::White),
            ))
        };
        let cursor_x = input_inner
            .x
            .saturating_add(line.width().min(u16::MAX as usize) as u16)
            .min(input_inner.right().saturating_sub(1));
        frame.render_widget(Paragraph::new(line), input_inner);
        frame.set_cursor_position(Position::new(cursor_x, input_inner.y));
    }
    if let (Some(error_area), Some(error)) = (error_area, state.error.as_deref()) {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("! {error}"),
                Style::default().fg(Color::Red),
            ))),
            error_area,
        );
    }

    let total = settings_row_count(state);
    let visible = rows_area.height as usize;
    let start = result_start(total, state.selected, visible);
    let end = (start + visible).min(total);
    let marker = marker_frames(state.config.marker.as_deref().unwrap_or(DEFAULT_MARKER))[0];
    let commands = configured_commands(&state.config);
    let items: Vec<ListItem> = (start..end)
        .map(|row| {
            let selected = row == state.selected;
            let marker_style = if selected {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let label_style = if selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let value_style = if selected {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let (label, value): (&str, String) = if row == SETTINGS_GLOBAL_ROW {
                (
                    "Global Mac search",
                    if global_search_enabled(&state.config) {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                )
            } else if row == SETTINGS_HIDDEN_ROW {
                (
                    "Include hidden folders",
                    if state.config.scan_hidden.unwrap_or(false) {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                )
            } else if row == SETTINGS_COFFEE_ROW {
                (
                    "Coffee reminder",
                    if state.config.coffee_enabled.unwrap_or(true) {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                )
            } else if row == SETTINGS_DEPTH_ROW {
                (
                    "Search depth",
                    depth_label(state.config.exact_depth).to_string(),
                )
            } else if row == SETTINGS_ADD_ROW {
                ("+ Add search folder", String::new())
            } else if row >= SETTINGS_PATH_START && row < settings_accent_row(state) {
                let path = &state.config.roots.as_ref().expect("path row has roots")
                    [row - SETTINGS_PATH_START];
                ("Folder", path.to_string_lossy().into_owned())
            } else if row == settings_accent_row(state) {
                (
                    "Accent color",
                    state.config.accent.as_deref().unwrap_or("cyan").to_string(),
                )
            } else if row == settings_marker_row(state) {
                (
                    "Selector marker",
                    marker_label(state.config.marker.as_deref().unwrap_or(DEFAULT_MARKER))
                        .to_string(),
                )
            } else if row == settings_command_add_row(state) {
                ("+ Add command preset", String::new())
            } else {
                let command = &commands[row - settings_command_start(state)];
                (
                    "Command",
                    format!("{} = {}", command.label, command.command),
                )
            };
            ListItem::new(Line::from(vec![
                Span::styled(if selected { marker } else { "  " }, marker_style),
                Span::styled(format!("{label:<24}"), label_style),
                Span::styled(value, value_style),
            ]))
        })
        .collect();
    frame.render_widget(List::new(items), rows_area);
    rows_area
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_exact_before_partial() {
        let a = PathBuf::from("/tmp/Mascotify");
        let b = PathBuf::from("/tmp/Mascotify Website");
        let result = rank(&[b, a.clone()], "mascotify");
        assert_eq!(result[0].0, a);
    }

    #[test]
    fn ranks_shallower_equal_matches_first() {
        let shallow = PathBuf::from("/tmp/projects/skills");
        let deep = PathBuf::from("/tmp/projects/realistic/skills");
        let result = rank(&[deep, shallow.clone()], "skills");
        assert_eq!(result[0].0, shallow);
    }

    #[test]
    fn depth_limit_includes_all_layers_through_limit() {
        let root = env::temp_dir().join(format!("wtfis-depth-test-{}", std::process::id()));
        let deepest = root.join("one/two/three/four");
        fs::create_dir_all(&deepest).expect("create depth test tree");

        let paths = scan_exact_depth(std::slice::from_ref(&root), false, 3);

        assert!(paths.contains(&root.join("one")));
        assert!(paths.contains(&root.join("one/two")));
        assert!(paths.contains(&root.join("one/two/three")));
        assert!(!paths.contains(&deepest));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fuzzy_handles_typo() {
        assert!(fuzzy_score(Path::new("/tmp/Mascotify"), "mascotfy").is_some());
    }

    #[test]
    fn multi_token_search_ignores_token_order() {
        let target = PathBuf::from("/tmp/Everthink/Clonethink/junk");
        let paths = [
            target.clone(),
            PathBuf::from("/tmp/Everthink/EverDesk/junk"),
        ];
        assert_eq!(rank(&paths, "junk clonethink")[0].0, target);
        assert_eq!(rank(&paths, "clonethink junk")[0].0, paths[0]);
    }

    #[test]
    fn multi_token_search_requires_every_token() {
        let paths = [
            PathBuf::from("/tmp/Everthink/Clonethink/junk"),
            PathBuf::from("/tmp/Everthink/EverDesk/junk"),
        ];
        let result = rank(&paths, "junk clonethink");
        assert_eq!(result.len(), 1);
        assert!(result[0].0.ends_with("Clonethink/junk"));
    }

    #[test]
    fn confident_match_auto_corrects_one_missing_letter() {
        let paths = [PathBuf::from("/tmp/Mascotify")];
        assert_eq!(
            confident_match(&paths, "Mascotfy"),
            Some(PathBuf::from("/tmp/Mascotify"))
        );
    }

    #[test]
    fn confident_match_refuses_ambiguous_typo() {
        let paths = [
            PathBuf::from("/tmp/Mascotify"),
            PathBuf::from("/tmp/Mascotify-Website"),
        ];
        assert_eq!(confident_match(&paths, "Mascotfy"), None);
    }

    #[test]
    fn confident_match_normalizes_path_separators() {
        let path = PathBuf::from("/Users/volodymurvasualkiw/GSpace/Project");
        assert_eq!(
            confident_match(
                std::slice::from_ref(&path),
                "/Users/volodymurvasualkiwGSpace"
            ),
            Some(path)
        );
    }

    #[test]
    fn confident_match_finds_collapsed_configured_root() {
        let path = PathBuf::from("/Users/volodymurvasualkiw/GSpace");
        assert_eq!(
            confident_match(std::slice::from_ref(&path), "/usersgspace"),
            Some(path)
        );
    }

    #[test]
    fn confident_match_does_not_use_parent_path_for_project_names() {
        let project = PathBuf::from("/Users/me/GSpace/Opensource/WTFIS-CLI");
        let child = project.join("src");
        assert_eq!(
            confident_match(&[project.clone(), child], "wtfis-cli"),
            Some(project)
        );
    }

    #[test]
    fn result_start_keeps_selected_row_visible() {
        assert_eq!(result_start(10, 0, 3), 0);
        assert_eq!(result_start(10, 4, 3), 2);
        assert_eq!(result_start(10, 9, 3), 7);
    }

    #[test]
    fn split_command_input_accepts_command_after_project() {
        assert_eq!(
            split_command_input("wtfis-cli /opencode"),
            ("wtfis-cli".to_string(), Some("/opencode".to_string()))
        );
    }

    #[test]
    fn split_command_input_keeps_paths_intact() {
        assert_eq!(
            split_command_input("/Users/me/GSpace"),
            ("/Users/me/GSpace".to_string(), None)
        );
    }
}
