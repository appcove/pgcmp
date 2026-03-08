use crate::App;
use crate::cli::InitArgs;
use crate::config::{Config, DatabaseType, DbConfig, TlsMode};
use crate::db::postgres::DbConnection;
use crate::memfs::MemFs;
use crate::schema::{SchemaObject, group_by_schema, generate_schema_file};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs},
};
use std::io;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;
use tui_textarea::{TextArea, CursorMove};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    NewDb,
    OldDb,
    Pull,
    Claude,
    Instruct,
    Migration,
    Setup,
}

const TABS: [Tab; 7] = [Tab::Setup, Tab::NewDb, Tab::OldDb, Tab::Pull, Tab::Claude, Tab::Instruct, Tab::Migration];

/// Default template for MIGRATION.sql files.
/// Must start with BEGIN TRANSACTION and end with ROLLBACK for safety.
const DEFAULT_MIGRATION_TEMPLATE: &str = "\
BEGIN TRANSACTION;

-- Add your migration SQL statements here

ROLLBACK;
";

/// Default template for INSTRUCT.md files.
/// This is extra instructions for Claude.
const DEFAULT_INSTRUCT_TEMPLATE: &str = "\
# Extra Instructions for Claude

Add any additional instructions for Claude here.
";

impl Tab {
    fn index(self) -> usize {
        TABS.iter().position(|&t| t == self).unwrap_or(0)
    }

    fn label(self) -> &'static str {
        match self {
            Tab::NewDb => "New DB",
            Tab::OldDb => "Old DB",
            Tab::Pull => "Pull",
            Tab::Claude => "Claude",
            Tab::Instruct => "Instruct",
            Tab::Migration => "Migration",
            Tab::Setup => "Setup",
        }
    }

    fn next(self) -> Self {
        let idx = self.index();
        TABS[(idx + 1) % TABS.len()]
    }

    fn prev(self) -> Self {
        let idx = self.index();
        TABS[(idx + TABS.len() - 1) % TABS.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Host,
    Port,
    User,
    Password,
    Database,
    Tls,
}

const FIELDS: [Field; 6] = [
    Field::Host,
    Field::Port,
    Field::User,
    Field::Password,
    Field::Database,
    Field::Tls,
];

impl Field {
    fn next(self) -> Self {
        let idx = FIELDS.iter().position(|&f| f == self).unwrap_or(0);
        FIELDS[(idx + 1) % FIELDS.len()]
    }
    fn prev(self) -> Self {
        let idx = FIELDS.iter().position(|&f| f == self).unwrap_or(0);
        FIELDS[(idx + FIELDS.len() - 1) % FIELDS.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DialogButton {
    SaveAndClose,
    Discard,
    Cancel,
}

impl DialogButton {
    fn next(self) -> Self {
        match self {
            DialogButton::SaveAndClose => DialogButton::Discard,
            DialogButton::Discard => DialogButton::Cancel,
            DialogButton::Cancel => DialogButton::SaveAndClose,
        }
    }
    fn prev(self) -> Self {
        match self {
            DialogButton::SaveAndClose => DialogButton::Cancel,
            DialogButton::Discard => DialogButton::SaveAndClose,
            DialogButton::Cancel => DialogButton::Discard,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BottomButton {
    Save,
    Cancel,
}

impl BottomButton {
    fn toggle(self) -> Self {
        match self {
            BottomButton::Save => BottomButton::Cancel,
            BottomButton::Cancel => BottomButton::Save,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PullButton {
    Pull,
    Clear,
}

const PULL_BUTTONS: [PullButton; 2] = [PullButton::Pull, PullButton::Clear];

impl PullButton {
    fn next(self) -> Self {
        let idx = PULL_BUTTONS.iter().position(|&b| b == self).unwrap_or(0);
        PULL_BUTTONS[(idx + 1) % PULL_BUTTONS.len()]
    }
    fn prev(self) -> Self {
        let idx = PULL_BUTTONS.iter().position(|&b| b == self).unwrap_or(0);
        PULL_BUTTONS[(idx + PULL_BUTTONS.len() - 1) % PULL_BUTTONS.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MigrationButton {
    Clear,
    UndoClear,
    ViewContents,
}

const MIGRATION_BUTTONS: [MigrationButton; 3] = [
    MigrationButton::ViewContents,
    MigrationButton::Clear,
    MigrationButton::UndoClear,
];

impl MigrationButton {
    fn next(self) -> Self {
        let idx = MIGRATION_BUTTONS.iter().position(|&b| b == self).unwrap_or(0);
        MIGRATION_BUTTONS[(idx + 1) % MIGRATION_BUTTONS.len()]
    }
    fn prev(self) -> Self {
        let idx = MIGRATION_BUTTONS.iter().position(|&b| b == self).unwrap_or(0);
        MIGRATION_BUTTONS[(idx + MIGRATION_BUTTONS.len() - 1) % MIGRATION_BUTTONS.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClaudeButton {
    Generate,
    Revert,
    ViewContents,
}

const CLAUDE_BUTTONS: [ClaudeButton; 3] = [
    ClaudeButton::ViewContents,
    ClaudeButton::Generate,
    ClaudeButton::Revert,
];

impl ClaudeButton {
    fn next(self) -> Self {
        let idx = CLAUDE_BUTTONS.iter().position(|&b| b == self).unwrap_or(0);
        CLAUDE_BUTTONS[(idx + 1) % CLAUDE_BUTTONS.len()]
    }
    fn prev(self) -> Self {
        let idx = CLAUDE_BUTTONS.iter().position(|&b| b == self).unwrap_or(0);
        CLAUDE_BUTTONS[(idx + CLAUDE_BUTTONS.len() - 1) % CLAUDE_BUTTONS.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum InstructButton {
    Save,
    Revert,
}

const INSTRUCT_BUTTONS: [InstructButton; 2] = [
    InstructButton::Save,
    InstructButton::Revert,
];

impl InstructButton {
    fn next(self) -> Self {
        let idx = INSTRUCT_BUTTONS.iter().position(|&b| b == self).unwrap_or(0);
        INSTRUCT_BUTTONS[(idx + 1) % INSTRUCT_BUTTONS.len()]
    }
    fn prev(self) -> Self {
        let idx = INSTRUCT_BUTTONS.iter().position(|&b| b == self).unwrap_or(0);
        INSTRUCT_BUTTONS[(idx + INSTRUCT_BUTTONS.len() - 1) % INSTRUCT_BUTTONS.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GitButton {
    Init,
    CommitAll,
    Refresh,
}

const GIT_BUTTONS: [GitButton; 3] = [GitButton::Init, GitButton::CommitAll, GitButton::Refresh];

impl GitButton {
    fn next(self) -> Self {
        let idx = GIT_BUTTONS.iter().position(|&b| b == self).unwrap_or(0);
        GIT_BUTTONS[(idx + 1) % GIT_BUTTONS.len()]
    }
    fn prev(self) -> Self {
        let idx = GIT_BUTTONS.iter().position(|&b| b == self).unwrap_or(0);
        GIT_BUTTONS[(idx + GIT_BUTTONS.len() - 1) % GIT_BUTTONS.len()]
    }
}

#[derive(Default)]
struct DbForm {
    host: Input,
    port: Input,
    user: Input,
    password: Input,
    database: Input,
    tls: TlsMode,
    validation_status: String,
    connection_status: String,
    saved_config: Option<DbConfig>,
    last_connected_config: Option<DbConfig>,
}

impl DbForm {
    fn new() -> Self {
        Self {
            host: Input::default().with_value("localhost".into()),
            port: Input::default().with_value("5432".into()),
            user: Input::default(),
            password: Input::default(),
            database: Input::default(),
            tls: TlsMode::default(),
            validation_status: "Not validated".into(),
            connection_status: "(not connected)".into(),
            saved_config: None,
            last_connected_config: None,
        }
    }

    fn from_config(config: &DbConfig) -> Self {
        Self {
            host: Input::default().with_value(config.host().to_string()),
            port: Input::default().with_value(config.port().to_string()),
            user: Input::default().with_value(config.user().to_string()),
            password: Input::default().with_value(config.password().to_string()),
            database: Input::default().with_value(config.database().to_string()),
            tls: config.tls(),
            validation_status: "Not validated".into(),
            connection_status: "(not connected)".into(),
            saved_config: Some(config.clone()),
            last_connected_config: None,
        }
    }

    fn needs_connection_test(&self) -> bool {
        match (&self.last_connected_config, self.to_config()) {
            (Some(last), Some(current)) => {
                last.host() != current.host()
                    || last.port() != current.port()
                    || last.user() != current.user()
                    || last.password() != current.password()
                    || last.database() != current.database()
                    || last.tls() != current.tls()
            }
            (None, Some(_)) => true,
            _ => false,
        }
    }

    fn to_config(&self) -> Option<DbConfig> {
        let port: u16 = self.port.value().parse().ok()?;
        let password = self.password.value();
        Some(DbConfig {
            host: Some(self.host.value().into()),
            port: Some(port),
            user: Some(self.user.value().into()),
            password: if password.is_empty() {
                None
            } else {
                Some(password.into())
            },
            database: Some(self.database.value().into()),
            tls: Some(self.tls),
        })
    }

    fn to_config_partial(&self) -> DbConfig {
        let host = self.host.value();
        let port = self.port.value().parse().ok();
        let user = self.user.value();
        let password = self.password.value();
        let database = self.database.value();
        DbConfig {
            host: if host.is_empty() { None } else { Some(host.into()) },
            port,
            user: if user.is_empty() { None } else { Some(user.into()) },
            password: if password.is_empty() { None } else { Some(password.into()) },
            database: if database.is_empty() { None } else { Some(database.into()) },
            tls: Some(self.tls),
        }
    }

    fn validate(&mut self) -> bool {
        let mut errors = Vec::new();

        if self.host.value().is_empty() {
            errors.push("Host required");
        }

        let port_val = self.port.value();
        if port_val.is_empty() {
            errors.push("Port required");
        } else if port_val.parse::<u16>().is_err() {
            errors.push("Port invalid");
        }

        if self.user.value().is_empty() {
            errors.push("User required");
        }

        if self.database.value().is_empty() {
            errors.push("Database required");
        }

        if errors.is_empty() {
            self.validation_status = "Valid".into();
            true
        } else {
            self.validation_status = errors.join(", ");
            self.connection_status = "(not connected)".into();
            false
        }
    }

    fn is_saved(&self) -> bool {
        match &self.saved_config {
            None => false,
            Some(saved) => {
                self.host.value() == saved.host()
                    && self.port.value() == saved.port().to_string()
                    && self.user.value() == saved.user()
                    && self.password.value() == saved.password()
                    && self.database.value() == saved.database()
                    && self.tls == saved.tls()
            }
        }
    }

    fn input_mut(&mut self, field: Field) -> Option<&mut Input> {
        match field {
            Field::Host => Some(&mut self.host),
            Field::Port => Some(&mut self.port),
            Field::User => Some(&mut self.user),
            Field::Password => Some(&mut self.password),
            Field::Database => Some(&mut self.database),
            Field::Tls => None,
        }
    }

    fn is_connected(&self) -> bool {
        self.connection_status.contains("Connected")
    }
}

type ConnectionResult = Result<DbConfig, String>;

/// Focus state for different tabs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    TabBar,
    SetupDbType,
    DbField(Field),
    PullButton(PullButton),
    ClaudeButton(ClaudeButton),
    InstructTextArea,
    InstructButton(InstructButton),
    MigrationButton(MigrationButton),
    GitButton(GitButton),
    BottomButton(BottomButton),
}

struct State {
    app: &'static App,
    tab: Tab,
    focus: Focus,
    database_type: DatabaseType,
    new_db: DbForm,
    old_db: DbForm,
    memfs: MemFs,
    click_areas: Vec<(ClickTarget, Rect)>,
    new_db_connection_rx: Option<oneshot::Receiver<ConnectionResult>>,
    old_db_connection_rx: Option<oneshot::Receiver<ConnectionResult>>,
    dialog: Option<DialogButton>,
    /// Status message for pull operations
    pull_status: String,
    /// Is a pull operation in progress?
    pull_in_progress: bool,
    /// Receiver for pull messages (status updates and final result)
    pull_rx: Option<mpsc::Receiver<PullMessage>>,
    /// Cancellation token for pull operation
    pull_cancel: Option<CancellationToken>,
    /// Migration contents dialog open
    migration_dialog_open: bool,
    /// Scroll offset for migration content dialog
    migration_scroll: u16,
    /// Claude contents dialog open
    claude_dialog_open: bool,
    /// Scroll offset for claude content dialog
    claude_scroll: u16,
    /// Instruct contents dialog open (view mode)
    instruct_dialog_open: bool,
    /// Scroll offset for instruct content dialog
    instruct_scroll: u16,
    /// Instruct textarea for inline editing
    instruct_textarea: TextArea<'static>,
    /// Flag to indicate the app should exit
    should_exit: bool,
    /// Whether .git directory exists
    has_git: bool,
    /// Whether git status is clean (no uncommitted changes)
    git_clean: bool,
    /// Git status output lines
    git_status_lines: Vec<String>,
    /// Git branch name
    git_branch: String,
}

/// Messages sent during pull operation
enum PullMessage {
    /// Status update (displayed to user)
    Status(String),
    /// Final result with schema objects
    Done(Result<PullData, String>),
}

#[derive(Default)]
struct PullData {
    new_objects: Vec<SchemaObject>,
    old_objects: Vec<SchemaObject>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClickTarget {
    Tab(Tab),
    Field(Tab, Field),
    DbTypeOption(DatabaseType),
    TlsOption(Tab, TlsMode),
    PullButton(PullButton),
    ClaudeButton(ClaudeButton),
    InstructButton(InstructButton),
    MigrationButton(MigrationButton),
    GitButton(GitButton),
    BottomButton(BottomButton),
    DialogButton(DialogButton),
}

impl State {
    fn new(app: &'static App) -> Self {
        let config = Config::load_or_default(&app.path);
        let new_db = match &config.new {
            Some(c) => DbForm::from_config(c),
            None => DbForm::new(),
        };
        let old_db = match &config.old {
            Some(c) => DbForm::from_config(c),
            None => DbForm::new(),
        };

        let mut memfs = MemFs::new(&app.path);

        // Add .gitignore if it doesn't exist on disk
        let gitignore_path = app.path.join(".gitignore");
        if !gitignore_path.exists() {
            memfs.write(
                PathBuf::from(".gitignore"),
                "# pgcmp generated gitignore\nCONFIG.toml\n",
            );
        }

        // Add .claudeignore if it doesn't exist on disk
        let claudeignore_path = app.path.join(".claudeignore");
        if !claudeignore_path.exists() {
            memfs.write(
                PathBuf::from(".claudeignore"),
                "# Files Claude should ignore\nCONFIG.toml\n",
            );
        }

        // Add MIGRATION.sql if it doesn't exist on disk
        let migration_path = app.path.join("MIGRATION.sql");
        if !migration_path.exists() {
            memfs.write(
                PathBuf::from("MIGRATION.sql"),
                DEFAULT_MIGRATION_TEMPLATE,
            );
        }

        // Add INSTRUCT.md if it doesn't exist on disk
        let instruct_path = app.path.join("INSTRUCT.md");
        if !instruct_path.exists() {
            memfs.write(
                PathBuf::from("INSTRUCT.md"),
                DEFAULT_INSTRUCT_TEMPLATE,
            );
        }

        Self {
            app,
            tab: Tab::Setup,
            focus: Focus::TabBar,
            database_type: config.database_type,
            new_db,
            old_db,
            memfs,
            click_areas: Vec::new(),
            new_db_connection_rx: None,
            old_db_connection_rx: None,
            dialog: None,
            pull_status: String::new(),
            pull_in_progress: false,
            pull_rx: None,
            pull_cancel: None,
            migration_dialog_open: false,
            migration_scroll: 0,
            claude_dialog_open: false,
            claude_scroll: 0,
            instruct_dialog_open: false,
            instruct_scroll: 0,
            instruct_textarea: TextArea::default(),
            should_exit: false,
            has_git: false,
            git_clean: true,
            git_status_lines: Vec::new(),
            git_branch: String::new(),
        }
    }

    fn refresh_git_status(&mut self) {
        let git_dir = self.app.path.join(".git");
        self.has_git = git_dir.exists() && git_dir.is_dir();

        if !self.has_git {
            self.git_clean = true;
            self.git_status_lines.clear();
            self.git_branch.clear();
            return;
        }

        // Get branch name
        if let Ok(output) = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&self.app.path)
            .output()
        {
            self.git_branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }

        // Get status
        if let Ok(output) = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.app.path)
            .output()
        {
            let status = String::from_utf8_lossy(&output.stdout);
            self.git_status_lines = status.lines().map(|s| s.to_string()).collect();
            self.git_clean = self.git_status_lines.is_empty();
        }
    }

    fn has_unsaved_changes(&self) -> bool {
        !self.new_db.is_saved() || !self.old_db.is_saved() || self.memfs.file_count() > 0
    }

    fn current_form(&mut self) -> Option<&mut DbForm> {
        match self.tab {
            Tab::NewDb => Some(&mut self.new_db),
            Tab::OldDb => Some(&mut self.old_db),
            _ => None,
        }
    }

    fn migration_in_memfs(&self) -> bool {
        self.memfs.contains(PathBuf::from("MIGRATION.sql"))
    }

    /// Get migration file content (from memfs if staged, otherwise from disk)
    fn migration_content(&self) -> String {
        // First check memfs
        if let Some(content) = self.memfs.get(PathBuf::from("MIGRATION.sql")) {
            return content.to_string();
        }
        // Fall back to disk
        let path = self.app.path.join("MIGRATION.sql");
        std::fs::read_to_string(&path).unwrap_or_default()
    }

    fn claude_in_memfs(&self) -> bool {
        self.memfs.contains(PathBuf::from("CLAUDE.md"))
    }

    /// Get CLAUDE.md content (from memfs if staged, otherwise from disk)
    fn claude_content(&self) -> String {
        // First check memfs
        if let Some(content) = self.memfs.get(PathBuf::from("CLAUDE.md")) {
            return content.to_string();
        }
        // Fall back to disk
        let path = self.app.path.join("CLAUDE.md");
        std::fs::read_to_string(&path).unwrap_or_default()
    }

    fn instruct_in_memfs(&self) -> bool {
        self.memfs.contains(PathBuf::from("INSTRUCT.md"))
    }

    /// Get INSTRUCT.md content (from memfs if staged, otherwise from disk)
    fn instruct_content(&self) -> String {
        // First check memfs
        if let Some(content) = self.memfs.get(PathBuf::from("INSTRUCT.md")) {
            return content.to_string();
        }
        // Fall back to disk
        let path = self.app.path.join("INSTRUCT.md");
        std::fs::read_to_string(&path).unwrap_or_default()
    }

    /// Check if new database has been pulled (has files in memfs under new.database/)
    fn has_new_pulled(&self) -> bool {
        self.memfs.list_files().iter().any(|(path, is_write)| {
            *is_write && path.starts_with("new.database/")
        })
    }

    /// Check if old database has been pulled (has files in memfs under old.database/)
    fn has_old_pulled(&self) -> bool {
        self.memfs.list_files().iter().any(|(path, is_write)| {
            *is_write && path.starts_with("old.database/")
        })
    }

    /// Check if both databases have been pulled
    fn has_both_pulled(&self) -> bool {
        self.has_new_pulled() && self.has_old_pulled()
    }
}

pub async fn run_init(app: &'static App, args: InitArgs) -> anyhow::Result<()> {
    if args.non_interactive {
        anyhow::bail!("Non-interactive mode not yet supported");
    }

    // Check if directory has git or is empty
    let git_dir = app.path.join(".git");
    let has_git = git_dir.exists() && git_dir.is_dir();

    if !has_git {
        // Check if directory is empty (excluding . and ..)
        let is_empty = std::fs::read_dir(&app.path)
            .map(|entries| entries.count() == 0)
            .unwrap_or(false);

        if !is_empty {
            anyhow::bail!(
                "Directory is not empty and does not contain a git repository.\n\
                You must either:\n\
                  1. Run 'git init' to initialize a git repository, or\n\
                  2. Use an empty directory\n\n\
                This requirement ensures your work is tracked in version control."
            );
        }
    }

    run_ui(app).await
}

async fn run_ui(app: &'static App) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let result = main_loop(&mut terminal, app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn main_loop(
    term: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &'static App,
) -> anyhow::Result<()> {
    let mut state = State::new(app);

    // Check git status
    state.refresh_git_status();

    // Auto-test connections if config was loaded
    if state.new_db.saved_config.is_some() {
        start_connection_test_for(&mut state, Tab::NewDb);
    }
    if state.old_db.saved_config.is_some() {
        start_connection_test_for(&mut state, Tab::OldDb);
    }

    loop {
        poll_connection_tasks(&mut state);
        poll_pull_task(&mut state);

        term.draw(|f| ui(f, &mut state))?;

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        let evt = event::read()?;

        // Handle pull dialog (when pull is in progress)
        if state.pull_in_progress {
            handle_pull_dialog_event(&evt, &mut state);
            continue;
        }

        // Handle migration contents dialog
        if state.migration_dialog_open {
            handle_migration_dialog_event(&evt, &mut state);
            continue;
        }

        // Handle Claude contents dialog
        if state.claude_dialog_open {
            handle_claude_dialog_event(&evt, &mut state);
            continue;
        }

        // Handle Instruct contents dialog
        if state.instruct_dialog_open {
            handle_instruct_dialog_event(&evt, &mut state);
            continue;
        }

        // Handle unsaved changes dialog
        if state.dialog.is_some() {
            if handle_dialog_event(&evt, &mut state)? {
                return Ok(());
            }
            continue;
        }

        // Normal event handling
        if handle_event(&evt, &mut state)? {
            return Ok(());
        }

        // Check if we should exit after event handling
        if state.should_exit {
            return Ok(());
        }
    }
}

fn handle_dialog_event(evt: &Event, state: &mut State) -> anyhow::Result<bool> {
    match evt {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            match key.code {
                KeyCode::Esc => {
                    state.dialog = None;
                }
                KeyCode::Tab | KeyCode::Right | KeyCode::Down => {
                    state.dialog = Some(state.dialog.unwrap().next());
                }
                KeyCode::BackTab | KeyCode::Left | KeyCode::Up => {
                    state.dialog = Some(state.dialog.unwrap().prev());
                }
                KeyCode::Enter | KeyCode::Char(' ') => match state.dialog.unwrap() {
                    DialogButton::SaveAndClose => {
                        if !state.git_clean {
                            state.pull_status = "Cannot save: git status is not clean. Commit or discard changes first.".to_string();
                            state.dialog = None;
                        } else {
                            save_all(&state.app, state.database_type, &mut state.new_db, &mut state.old_db, &state.memfs)?;
                            return Ok(true);
                        }
                    }
                    DialogButton::Discard => {
                        return Ok(true);
                    }
                    DialogButton::Cancel => {
                        state.dialog = None;
                    }
                },
                _ => {}
            }
        }
        Event::Mouse(m) if m.kind == MouseEventKind::Down(event::MouseButton::Left) => {
            let pos = Rect::new(m.column, m.row, 1, 1);
            let clicked = state
                .click_areas
                .iter()
                .find(|(_, rect)| rect.intersects(pos))
                .map(|(target, _)| *target);

            if let Some(ClickTarget::DialogButton(btn)) = clicked {
                match btn {
                    DialogButton::SaveAndClose => {
                        if !state.git_clean {
                            state.pull_status = "Cannot save: git status is not clean. Commit or discard changes first.".to_string();
                            state.dialog = None;
                        } else {
                            save_all(&state.app, state.database_type, &mut state.new_db, &mut state.old_db, &state.memfs)?;
                            return Ok(true);
                        }
                    }
                    DialogButton::Discard => {
                        return Ok(true);
                    }
                    DialogButton::Cancel => {
                        state.dialog = None;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_pull_dialog_event(evt: &Event, state: &mut State) {
    match evt {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            match key.code {
                KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C') => {
                    // Cancel the pull operation
                    if let Some(cancel) = &state.pull_cancel {
                        cancel.cancel();
                    }
                    state.pull_status = "Cancelled".into();
                    state.pull_in_progress = false;
                    state.pull_rx = None;
                    state.pull_cancel = None;
                }
                _ => {}
            }
        }
        Event::Mouse(m) if m.kind == MouseEventKind::Down(event::MouseButton::Left) => {
            // Check if cancel button was clicked
            let pos = Rect::new(m.column, m.row, 1, 1);
            let clicked = state
                .click_areas
                .iter()
                .find(|(_, rect)| rect.intersects(pos))
                .map(|(target, _)| *target);

            if let Some(ClickTarget::PullButton(_)) = clicked {
                // Cancel button clicked
                if let Some(cancel) = &state.pull_cancel {
                    cancel.cancel();
                }
                state.pull_status = "Cancelled".into();
                state.pull_in_progress = false;
                state.pull_rx = None;
                state.pull_cancel = None;
            }
        }
        _ => {}
    }
}

fn handle_migration_dialog_event(evt: &Event, state: &mut State) {
    let content = state.migration_content();
    let line_count = content.lines().count() as u16;

    match evt {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    state.migration_dialog_open = false;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if state.migration_scroll < line_count.saturating_sub(1) {
                        state.migration_scroll += 1;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.migration_scroll = state.migration_scroll.saturating_sub(1);
                }
                KeyCode::PageDown => {
                    state.migration_scroll = (state.migration_scroll + 20).min(line_count.saturating_sub(1));
                }
                KeyCode::PageUp => {
                    state.migration_scroll = state.migration_scroll.saturating_sub(20);
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    state.migration_scroll = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    state.migration_scroll = line_count.saturating_sub(1);
                }
                _ => {}
            }
        }
        Event::Mouse(m) => {
            match m.kind {
                MouseEventKind::Down(event::MouseButton::Left) => {
                    // Close on click outside could be added here
                }
                MouseEventKind::ScrollDown => {
                    if state.migration_scroll < line_count.saturating_sub(1) {
                        state.migration_scroll += 3;
                    }
                }
                MouseEventKind::ScrollUp => {
                    state.migration_scroll = state.migration_scroll.saturating_sub(3);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_claude_dialog_event(evt: &Event, state: &mut State) {
    let content = state.claude_content();
    let line_count = content.lines().count() as u16;

    match evt {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    state.claude_dialog_open = false;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if state.claude_scroll < line_count.saturating_sub(1) {
                        state.claude_scroll += 1;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.claude_scroll = state.claude_scroll.saturating_sub(1);
                }
                KeyCode::PageDown => {
                    state.claude_scroll = (state.claude_scroll + 20).min(line_count.saturating_sub(1));
                }
                KeyCode::PageUp => {
                    state.claude_scroll = state.claude_scroll.saturating_sub(20);
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    state.claude_scroll = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    state.claude_scroll = line_count.saturating_sub(1);
                }
                _ => {}
            }
        }
        Event::Mouse(m) => {
            match m.kind {
                MouseEventKind::Down(event::MouseButton::Left) => {
                    // Close on click outside could be added here
                }
                MouseEventKind::ScrollDown => {
                    if state.claude_scroll < line_count.saturating_sub(1) {
                        state.claude_scroll += 3;
                    }
                }
                MouseEventKind::ScrollUp => {
                    state.claude_scroll = state.claude_scroll.saturating_sub(3);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_instruct_dialog_event(evt: &Event, state: &mut State) {
    let content = state.instruct_content();
    let line_count = content.lines().count() as u16;

    match evt {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    state.instruct_dialog_open = false;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if state.instruct_scroll < line_count.saturating_sub(1) {
                        state.instruct_scroll += 1;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.instruct_scroll = state.instruct_scroll.saturating_sub(1);
                }
                KeyCode::PageDown => {
                    state.instruct_scroll = (state.instruct_scroll + 20).min(line_count.saturating_sub(1));
                }
                KeyCode::PageUp => {
                    state.instruct_scroll = state.instruct_scroll.saturating_sub(20);
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    state.instruct_scroll = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    state.instruct_scroll = line_count.saturating_sub(1);
                }
                _ => {}
            }
        }
        Event::Mouse(m) => {
            match m.kind {
                MouseEventKind::Down(event::MouseButton::Left) => {
                    // Close on click outside could be added here
                }
                MouseEventKind::ScrollDown => {
                    if state.instruct_scroll < line_count.saturating_sub(1) {
                        state.instruct_scroll += 3;
                    }
                }
                MouseEventKind::ScrollUp => {
                    state.instruct_scroll = state.instruct_scroll.saturating_sub(3);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_event(evt: &Event, state: &mut State) -> anyhow::Result<bool> {
    // Special handling for textarea when focused
    if matches!(state.focus, Focus::InstructTextArea) {
        if let Event::Key(key) = evt {
            if key.kind == KeyEventKind::Press {
                use crossterm::event::KeyModifiers;
                // Handle Ctrl+S to save
                if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    let content = state.instruct_textarea.lines().join("\n");
                    state.memfs.write(PathBuf::from("INSTRUCT.md"), &content);
                    return Ok(false);
                }
                // Handle Tab, BackTab, Esc for navigation
                match key.code {
                    KeyCode::Tab => {
                        state.focus = Focus::InstructButton(InstructButton::Save);
                        return Ok(false);
                    }
                    KeyCode::BackTab => {
                        state.focus = Focus::TabBar;
                        return Ok(false);
                    }
                    KeyCode::Esc => {
                        if state.has_unsaved_changes() {
                            state.dialog = Some(DialogButton::SaveAndClose);
                        } else {
                            return Ok(true);
                        }
                        return Ok(false);
                    }
                    _ => {}
                }
            }
        }
        // Pass event to textarea
        state.instruct_textarea.input(evt.clone());
        return Ok(false);
    }

    match evt {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            match key.code {
                KeyCode::Esc => {
                    if state.has_unsaved_changes() {
                        state.dialog = Some(DialogButton::SaveAndClose);
                    } else {
                        return Ok(true);
                    }
                }
                _ => {
                    handle_tab_key_event(key.code, state)?;
                }
            }
        }
        Event::Mouse(m) if m.kind == MouseEventKind::Down(event::MouseButton::Left) => {
            handle_mouse_click(m.column, m.row, state)?;
        }
        _ => {}
    }
    Ok(false)
}

fn handle_tab_key_event(code: KeyCode, state: &mut State) -> anyhow::Result<()> {
    match state.focus {
        Focus::TabBar => {
            match code {
                KeyCode::Left => state.tab = state.tab.prev(),
                KeyCode::Right => state.tab = state.tab.next(),
                KeyCode::Tab | KeyCode::Down => {
                    // Move focus into tab content
                    state.focus = default_focus_for_tab(state.tab);
                }
                KeyCode::Enter => {
                    state.focus = default_focus_for_tab(state.tab);
                }
                _ => {}
            }
        }
        Focus::DbField(field) => {
            match code {
                KeyCode::Tab | KeyCode::Down => {
                    if field == Field::Tls {
                        // Move to bottom buttons
                        state.focus = Focus::BottomButton(BottomButton::Save);
                    } else {
                        state.focus = Focus::DbField(field.next());
                    }
                    // Validate and test connection when leaving field
                    if let Some(form) = state.current_form() {
                        if form.validate() {
                            start_connection_test_for(state, state.tab);
                        }
                    }
                }
                KeyCode::BackTab | KeyCode::Up => {
                    if field == Field::Host {
                        state.focus = Focus::TabBar;
                    } else {
                        state.focus = Focus::DbField(field.prev());
                    }
                    if let Some(form) = state.current_form() {
                        if form.validate() {
                            start_connection_test_for(state, state.tab);
                        }
                    }
                }
                KeyCode::Left | KeyCode::Right if field == Field::Tls => {
                    if let Some(form) = state.current_form() {
                        form.tls = form.tls.toggle();
                        start_connection_test_for(state, state.tab);
                    }
                }
                KeyCode::Enter | KeyCode::Char(' ') if field == Field::Tls => {
                    if let Some(form) = state.current_form() {
                        form.tls = form.tls.toggle();
                        start_connection_test_for(state, state.tab);
                    }
                }
                _ => {
                    // Forward to input handler
                    if let Some(form) = state.current_form() {
                        if let Some(input) = form.input_mut(field) {
                            let fake_evt = Event::Key(crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE));
                            input.handle_event(&fake_evt);
                        }
                    }
                }
            }
        }
        Focus::PullButton(btn) => {
            match code {
                KeyCode::Tab | KeyCode::Down => {
                    if btn == PullButton::Clear {
                        state.focus = Focus::BottomButton(BottomButton::Save);
                    } else {
                        state.focus = Focus::PullButton(btn.next());
                    }
                }
                KeyCode::BackTab | KeyCode::Up => {
                    if btn == PullButton::Pull {
                        state.focus = Focus::TabBar;
                    } else {
                        state.focus = Focus::PullButton(btn.prev());
                    }
                }
                KeyCode::Left => {
                    state.focus = Focus::PullButton(btn.prev());
                }
                KeyCode::Right => {
                    state.focus = Focus::PullButton(btn.next());
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    match btn {
                        PullButton::Pull => start_pull(state),
                        PullButton::Clear => clear_database_structure(state),
                    }
                }
                _ => {}
            }
        }
        Focus::ClaudeButton(btn) => {
            match code {
                KeyCode::Tab => {
                    if btn == ClaudeButton::Revert {
                        state.focus = Focus::BottomButton(BottomButton::Save);
                    } else {
                        state.focus = Focus::ClaudeButton(btn.next());
                    }
                }
                KeyCode::BackTab => {
                    if btn == ClaudeButton::ViewContents {
                        state.focus = Focus::TabBar;
                    } else {
                        state.focus = Focus::ClaudeButton(btn.prev());
                    }
                }
                KeyCode::Down => {
                    match btn {
                        ClaudeButton::ViewContents => {
                            state.focus = Focus::ClaudeButton(ClaudeButton::Generate);
                        }
                        _ => {
                            state.focus = Focus::BottomButton(BottomButton::Save);
                        }
                    }
                }
                KeyCode::Up => {
                    match btn {
                        ClaudeButton::Generate | ClaudeButton::Revert => {
                            state.focus = Focus::ClaudeButton(ClaudeButton::ViewContents);
                        }
                        ClaudeButton::ViewContents => {
                            state.focus = Focus::TabBar;
                        }
                    }
                }
                KeyCode::Left => {
                    state.focus = Focus::ClaudeButton(btn.prev());
                }
                KeyCode::Right => {
                    state.focus = Focus::ClaudeButton(btn.next());
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    match btn {
                        ClaudeButton::Generate => {
                            if state.has_both_pulled() {
                                let content = generate_claude_md(state);
                                state.memfs.write(PathBuf::from("CLAUDE.md"), content);
                            }
                        }
                        ClaudeButton::Revert => {
                            state.memfs.remove(PathBuf::from("CLAUDE.md"));
                        }
                        ClaudeButton::ViewContents => {
                            state.claude_dialog_open = true;
                            state.claude_scroll = 0;
                        }
                    }
                }
                _ => {}
            }
        }
        Focus::InstructTextArea => {
            match code {
                KeyCode::Tab => {
                    state.focus = Focus::InstructButton(InstructButton::Save);
                }
                KeyCode::BackTab => {
                    state.focus = Focus::TabBar;
                }
                KeyCode::Esc => {
                    state.focus = Focus::TabBar;
                }
                _ => {
                    // Pass key events to textarea
                    // Note: we handle this specially in handle_event
                }
            }
        }
        Focus::InstructButton(btn) => {
            match code {
                KeyCode::Tab => {
                    if btn == InstructButton::Revert {
                        state.focus = Focus::BottomButton(BottomButton::Save);
                    } else {
                        state.focus = Focus::InstructButton(btn.next());
                    }
                }
                KeyCode::BackTab => {
                    if btn == InstructButton::Save {
                        state.focus = Focus::InstructTextArea;
                    } else {
                        state.focus = Focus::InstructButton(btn.prev());
                    }
                }
                KeyCode::Down => {
                    state.focus = Focus::BottomButton(BottomButton::Save);
                }
                KeyCode::Up => {
                    state.focus = Focus::InstructTextArea;
                }
                KeyCode::Left => {
                    state.focus = Focus::InstructButton(btn.prev());
                }
                KeyCode::Right => {
                    state.focus = Focus::InstructButton(btn.next());
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    match btn {
                        InstructButton::Save => {
                            let content = state.instruct_textarea.lines().join("\n");
                            state.memfs.write(PathBuf::from("INSTRUCT.md"), &content);
                        }
                        InstructButton::Revert => {
                            // Reload content from disk/memfs into textarea
                            let content = state.instruct_content();
                            state.instruct_textarea = TextArea::from(content.lines());
                        }
                    }
                }
                _ => {}
            }
        }
        Focus::MigrationButton(btn) => {
            match code {
                KeyCode::Tab => {
                    // Cycle through all migration buttons, then to bottom
                    if btn == MigrationButton::UndoClear {
                        state.focus = Focus::BottomButton(BottomButton::Save);
                    } else {
                        state.focus = Focus::MigrationButton(btn.next());
                    }
                }
                KeyCode::BackTab => {
                    // Cycle back through migration buttons, then to tab bar
                    if btn == MigrationButton::ViewContents {
                        state.focus = Focus::TabBar;
                    } else {
                        state.focus = Focus::MigrationButton(btn.prev());
                    }
                }
                KeyCode::Down => {
                    // Move to next row of buttons or to bottom bar
                    match btn {
                        MigrationButton::ViewContents => {
                            state.focus = Focus::MigrationButton(MigrationButton::Clear);
                        }
                        _ => {
                            state.focus = Focus::BottomButton(BottomButton::Save);
                        }
                    }
                }
                KeyCode::Up => {
                    // Move to previous row or tab bar
                    match btn {
                        MigrationButton::Clear | MigrationButton::UndoClear => {
                            state.focus = Focus::MigrationButton(MigrationButton::ViewContents);
                        }
                        MigrationButton::ViewContents => {
                            state.focus = Focus::TabBar;
                        }
                    }
                }
                KeyCode::Left => {
                    state.focus = Focus::MigrationButton(btn.prev());
                }
                KeyCode::Right => {
                    state.focus = Focus::MigrationButton(btn.next());
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    match btn {
                        MigrationButton::Clear => {
                            // Write empty content to memfs
                            state.memfs.write(PathBuf::from("MIGRATION.sql"), DEFAULT_MIGRATION_TEMPLATE);
                        }
                        MigrationButton::UndoClear => {
                            // Remove from memfs to restore disk version
                            state.memfs.remove(PathBuf::from("MIGRATION.sql"));
                        }
                        MigrationButton::ViewContents => {
                            state.migration_dialog_open = true;
                            state.migration_scroll = 0;
                        }
                    }
                }
                _ => {}
            }
        }
        Focus::SetupDbType => {
            match code {
                KeyCode::Tab | KeyCode::Down => {
                    state.focus = Focus::GitButton(GitButton::Init);
                }
                KeyCode::BackTab | KeyCode::Up => {
                    state.focus = Focus::TabBar;
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
                    state.database_type = state.database_type.toggle();
                }
                _ => {}
            }
        }
        Focus::GitButton(btn) => {
            match code {
                KeyCode::Tab => {
                    // Cycle through git buttons, then to bottom
                    if btn == GitButton::Refresh {
                        state.focus = Focus::BottomButton(BottomButton::Save);
                    } else {
                        state.focus = Focus::GitButton(btn.next());
                    }
                }
                KeyCode::BackTab => {
                    // Cycle back through git buttons, then to db type
                    if btn == GitButton::Init {
                        state.focus = Focus::SetupDbType;
                    } else {
                        state.focus = Focus::GitButton(btn.prev());
                    }
                }
                KeyCode::Down => {
                    state.focus = Focus::BottomButton(BottomButton::Save);
                }
                KeyCode::Up => {
                    state.focus = Focus::SetupDbType;
                }
                KeyCode::Left => {
                    state.focus = Focus::GitButton(btn.prev());
                }
                KeyCode::Right => {
                    state.focus = Focus::GitButton(btn.next());
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    match btn {
                        GitButton::Init => {
                            if !state.has_git {
                                git_init(state);
                            }
                        }
                        GitButton::CommitAll => {
                            if state.has_git && !state.git_clean {
                                git_commit_all(state);
                            }
                        }
                        GitButton::Refresh => {
                            state.refresh_git_status();
                        }
                    }
                }
                _ => {}
            }
        }
        Focus::BottomButton(btn) => {
            match code {
                KeyCode::Tab | KeyCode::Down => {
                    state.focus = Focus::TabBar;
                }
                KeyCode::BackTab | KeyCode::Up => {
                    state.focus = default_focus_for_tab(state.tab);
                }
                KeyCode::Left | KeyCode::Right => {
                    state.focus = Focus::BottomButton(btn.toggle());
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    match btn {
                        BottomButton::Save => {
                            if !state.git_clean {
                                state.pull_status = "Cannot save: git status is not clean. Commit or discard changes first.".to_string();
                            } else {
                                save_all(&state.app, state.database_type, &mut state.new_db, &mut state.old_db, &state.memfs)?;
                                state.should_exit = true;
                            }
                        }
                        BottomButton::Cancel => {
                            if state.has_unsaved_changes() {
                                state.dialog = Some(DialogButton::SaveAndClose);
                            } else {
                                state.should_exit = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn default_focus_for_tab(tab: Tab) -> Focus {
    match tab {
        Tab::NewDb | Tab::OldDb => Focus::DbField(Field::Host),
        Tab::Pull => Focus::PullButton(PullButton::Pull),
        Tab::Claude => Focus::ClaudeButton(ClaudeButton::ViewContents),
        Tab::Instruct => Focus::InstructTextArea,
        Tab::Migration => Focus::MigrationButton(MigrationButton::ViewContents),
        Tab::Setup => Focus::SetupDbType,
    }
}

fn handle_mouse_click(col: u16, row: u16, state: &mut State) -> anyhow::Result<()> {
    let pos = Rect::new(col, row, 1, 1);
    let clicked = state
        .click_areas
        .iter()
        .find(|(_, rect)| rect.intersects(pos))
        .map(|(target, _)| *target);

    if let Some(target) = clicked {
        match target {
            ClickTarget::Tab(tab) => {
                state.tab = tab;
                state.focus = default_focus_for_tab(tab);
            }
            ClickTarget::Field(tab, field) => {
                state.tab = tab;
                state.focus = Focus::DbField(field);
            }
            ClickTarget::DbTypeOption(db_type) => {
                state.focus = Focus::SetupDbType;
                state.database_type = db_type;
            }
            ClickTarget::TlsOption(tab, mode) => {
                state.tab = tab;
                state.focus = Focus::DbField(Field::Tls);
                if let Some(form) = state.current_form() {
                    if form.tls != mode {
                        form.tls = mode;
                        start_connection_test_for(state, tab);
                    }
                }
            }
            ClickTarget::PullButton(btn) => {
                state.focus = Focus::PullButton(btn);
                match btn {
                    PullButton::Pull => start_pull(state),
                    PullButton::Clear => clear_database_structure(state),
                }
            }
            ClickTarget::ClaudeButton(btn) => {
                state.focus = Focus::ClaudeButton(btn);
                match btn {
                    ClaudeButton::Generate => {
                        if state.has_both_pulled() {
                            let content = generate_claude_md(state);
                            state.memfs.write(PathBuf::from("CLAUDE.md"), content);
                        }
                    }
                    ClaudeButton::Revert => {
                        state.memfs.remove(PathBuf::from("CLAUDE.md"));
                    }
                    ClaudeButton::ViewContents => {
                        state.claude_dialog_open = true;
                        state.claude_scroll = 0;
                    }
                }
            }
            ClickTarget::InstructButton(btn) => {
                state.focus = Focus::InstructButton(btn);
                match btn {
                    InstructButton::Save => {
                        let content = state.instruct_textarea.lines().join("\n");
                        state.memfs.write(PathBuf::from("INSTRUCT.md"), &content);
                    }
                    InstructButton::Revert => {
                        // Reload content from disk/memfs into textarea
                        let content = state.instruct_content();
                        state.instruct_textarea = TextArea::from(content.lines());
                    }
                }
            }
            ClickTarget::MigrationButton(btn) => {
                state.focus = Focus::MigrationButton(btn);
                match btn {
                    MigrationButton::Clear => {
                        state.memfs.write(PathBuf::from("MIGRATION.sql"), DEFAULT_MIGRATION_TEMPLATE);
                    }
                    MigrationButton::UndoClear => {
                        state.memfs.remove(PathBuf::from("MIGRATION.sql"));
                    }
                    MigrationButton::ViewContents => {
                        state.migration_dialog_open = true;
                        state.migration_scroll = 0;
                    }
                }
            }
            ClickTarget::GitButton(btn) => {
                state.focus = Focus::GitButton(btn);
                match btn {
                    GitButton::Init => {
                        if !state.has_git {
                            git_init(state);
                        }
                    }
                    GitButton::CommitAll => {
                        if state.has_git && !state.git_clean {
                            git_commit_all(state);
                        }
                    }
                    GitButton::Refresh => {
                        state.refresh_git_status();
                    }
                }
            }
            ClickTarget::BottomButton(btn) => {
                state.focus = Focus::BottomButton(btn);
                match btn {
                    BottomButton::Save => {
                        if !state.git_clean {
                            state.pull_status = "Cannot save: git status is not clean. Commit or discard changes first.".to_string();
                        } else {
                            save_all(&state.app, state.database_type, &mut state.new_db, &mut state.old_db, &state.memfs)?;
                            state.should_exit = true;
                        }
                    }
                    BottomButton::Cancel => {
                        if state.has_unsaved_changes() {
                            state.dialog = Some(DialogButton::SaveAndClose);
                        } else {
                            state.should_exit = true;
                        }
                    }
                }
            }
            ClickTarget::DialogButton(_) => {}
        }
    }
    Ok(())
}

fn start_connection_test_for(state: &mut State, tab: Tab) {
    // Drop any pending receiver
    match tab {
        Tab::NewDb => state.new_db_connection_rx = None,
        Tab::OldDb => state.old_db_connection_rx = None,
        _ => return,
    };

    let form = match tab {
        Tab::NewDb => &mut state.new_db,
        Tab::OldDb => &mut state.old_db,
        _ => return,
    };

    if !form.validate() {
        form.last_connected_config = None;
        return;
    }

    if !form.needs_connection_test() {
        return;
    }

    form.last_connected_config = None;

    let config = match form.to_config() {
        Some(c) => c,
        None => {
            form.connection_status = "Invalid configuration".into();
            return;
        }
    };

    form.connection_status = "Testing...".into();

    let (tx, rx) = oneshot::channel();
    let connection_string = config.connection_string();
    tokio::spawn(async move {
        let result = match DbConnection::connect(&connection_string).await {
            Ok(_) => Ok(config),
            Err(e) => Err(format!("Failed: {}", e)),
        };
        let _ = tx.send(result);
    });

    match tab {
        Tab::NewDb => state.new_db_connection_rx = Some(rx),
        Tab::OldDb => state.old_db_connection_rx = Some(rx),
        _ => {}
    };
}

fn poll_connection_tasks(state: &mut State) {
    if let Some(rx) = &mut state.new_db_connection_rx {
        match rx.try_recv() {
            Ok(Ok(config)) => {
                state.new_db.connection_status = "Connected!".into();
                state.new_db.last_connected_config = Some(config);
                state.new_db_connection_rx = None;
            }
            Ok(Err(e)) => {
                state.new_db.connection_status = e;
                state.new_db.last_connected_config = None;
                state.new_db_connection_rx = None;
            }
            Err(oneshot::error::TryRecvError::Empty) => {}
            Err(oneshot::error::TryRecvError::Closed) => {
                state.new_db_connection_rx = None;
            }
        }
    }

    if let Some(rx) = &mut state.old_db_connection_rx {
        match rx.try_recv() {
            Ok(Ok(config)) => {
                state.old_db.connection_status = "Connected!".into();
                state.old_db.last_connected_config = Some(config);
                state.old_db_connection_rx = None;
            }
            Ok(Err(e)) => {
                state.old_db.connection_status = e;
                state.old_db.last_connected_config = None;
                state.old_db_connection_rx = None;
            }
            Err(oneshot::error::TryRecvError::Empty) => {}
            Err(oneshot::error::TryRecvError::Closed) => {
                state.old_db_connection_rx = None;
            }
        }
    }
}

fn start_pull(state: &mut State) {
    if state.pull_in_progress {
        return;
    }

    // Check connections
    if !state.new_db.is_connected() {
        state.pull_status = "New DB not connected".into();
        return;
    }
    if !state.old_db.is_connected() {
        state.pull_status = "Old DB not connected".into();
        return;
    }

    let new_config = state.new_db.to_config();
    let old_config = state.old_db.to_config();

    state.pull_in_progress = true;
    state.pull_status = "Starting pull...".into();

    let (tx, rx) = mpsc::channel(32);
    let cancel_token = CancellationToken::new();
    let cancel_clone = cancel_token.clone();

    tokio::spawn(async move {
        do_pull(new_config, old_config, tx, cancel_clone).await;
    });

    state.pull_rx = Some(rx);
    state.pull_cancel = Some(cancel_token);
}

fn clear_database_structure(state: &mut State) {
    // Remove all files under new.database/ and old.database/
    let files_to_remove: Vec<PathBuf> = state
        .memfs
        .list_files()
        .iter()
        .filter(|(path, is_write)| {
            *is_write && (path.starts_with("new.database/") || path.starts_with("old.database/"))
        })
        .map(|(path, _)| path.to_path_buf())
        .collect();

    for path in files_to_remove {
        state.memfs.remove(&path);
    }

    // Clear the directories
    state.memfs.clear_dir(PathBuf::from("new.database"));
    state.memfs.clear_dir(PathBuf::from("old.database"));

    // Zero out CLAUDE.md
    state.memfs.write(PathBuf::from("CLAUDE.md"), "");

    state.pull_status = "Database structure cleared".into();
}

fn git_init(state: &mut State) {
    let result = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&state.app.path)
        .output();

    match result {
        Ok(output) if output.status.success() => {
            state.pull_status = "Git repository initialized".into();
            state.refresh_git_status();
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            state.pull_status = format!("Git init failed: {}", stderr.trim());
        }
        Err(e) => {
            state.pull_status = format!("Git init error: {}", e);
        }
    }
}

fn git_commit_all(state: &mut State) {
    // First, stage all changes
    let add_result = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&state.app.path)
        .output();

    if let Err(e) = add_result {
        state.pull_status = format!("Git add error: {}", e);
        return;
    }

    // Commit with a default message
    let commit_result = std::process::Command::new("git")
        .args(["commit", "-m", "pgcmp: auto-commit changes"])
        .current_dir(&state.app.path)
        .output();

    match commit_result {
        Ok(output) if output.status.success() => {
            state.pull_status = "Changes committed successfully".into();
            state.refresh_git_status();
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("nothing to commit") || stderr.contains("nothing to commit") {
                state.pull_status = "Nothing to commit".into();
            } else {
                state.pull_status = format!("Git commit failed: {}", stderr.trim());
            }
            state.refresh_git_status();
        }
        Err(e) => {
            state.pull_status = format!("Git commit error: {}", e);
        }
    }
}

async fn do_pull(
    new_config: Option<DbConfig>,
    old_config: Option<DbConfig>,
    tx: mpsc::Sender<PullMessage>,
    cancel: CancellationToken,
) {
    let mut data = PullData::default();

    // Helper macro to check cancellation
    macro_rules! check_cancel {
        () => {
            if cancel.is_cancelled() {
                return;
            }
        };
    }

    if let Some(config) = new_config {
        check_cancel!();
        let _ = tx.send(PullMessage::Status("Connecting to new DB...".into())).await;

        let conn = match DbConnection::connect(&config.connection_string()).await {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("New DB connection failed: {}", e)))).await;
                return;
            }
        };

        check_cancel!();
        let _ = tx.send(PullMessage::Status("New DB: extracting types...".into())).await;
        match crate::db::postgres::types::extract_types(conn.client()).await {
            Ok(objs) => data.new_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract types: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("New DB: extracting tables...".into())).await;
        match crate::db::postgres::tables::extract_tables(conn.client()).await {
            Ok(objs) => data.new_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract tables: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("New DB: extracting views...".into())).await;
        match crate::db::postgres::views::extract_views(conn.client()).await {
            Ok(objs) => data.new_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract views: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("New DB: extracting materialized views...".into())).await;
        match crate::db::postgres::views::extract_materialized_views(conn.client()).await {
            Ok(objs) => data.new_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract materialized views: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("New DB: extracting functions...".into())).await;
        match crate::db::postgres::functions::extract_functions(conn.client()).await {
            Ok(objs) => data.new_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract functions: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("New DB: extracting indexes...".into())).await;
        match crate::db::postgres::indexes::extract_indexes(conn.client()).await {
            Ok(objs) => data.new_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract indexes: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("New DB: extracting constraints...".into())).await;
        match crate::db::postgres::constraints::extract_constraints(conn.client()).await {
            Ok(objs) => data.new_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract constraints: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("New DB: extracting triggers...".into())).await;
        match crate::db::postgres::triggers::extract_triggers(conn.client()).await {
            Ok(objs) => data.new_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract triggers: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("New DB: extracting sequences...".into())).await;
        match crate::db::postgres::sequences::extract_sequences(conn.client()).await {
            Ok(objs) => data.new_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract sequences: {}", e)))).await;
                return;
            }
        }

        let _ = tx.send(PullMessage::Status(format!("New DB: extracted {} objects", data.new_objects.len()))).await;
    }

    if let Some(config) = old_config {
        check_cancel!();
        let _ = tx.send(PullMessage::Status("Connecting to old DB...".into())).await;

        let conn = match DbConnection::connect(&config.connection_string()).await {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Old DB connection failed: {}", e)))).await;
                return;
            }
        };

        check_cancel!();
        let _ = tx.send(PullMessage::Status("Old DB: extracting types...".into())).await;
        match crate::db::postgres::types::extract_types(conn.client()).await {
            Ok(objs) => data.old_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract types: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("Old DB: extracting tables...".into())).await;
        match crate::db::postgres::tables::extract_tables(conn.client()).await {
            Ok(objs) => data.old_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract tables: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("Old DB: extracting views...".into())).await;
        match crate::db::postgres::views::extract_views(conn.client()).await {
            Ok(objs) => data.old_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract views: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("Old DB: extracting materialized views...".into())).await;
        match crate::db::postgres::views::extract_materialized_views(conn.client()).await {
            Ok(objs) => data.old_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract materialized views: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("Old DB: extracting functions...".into())).await;
        match crate::db::postgres::functions::extract_functions(conn.client()).await {
            Ok(objs) => data.old_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract functions: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("Old DB: extracting indexes...".into())).await;
        match crate::db::postgres::indexes::extract_indexes(conn.client()).await {
            Ok(objs) => data.old_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract indexes: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("Old DB: extracting constraints...".into())).await;
        match crate::db::postgres::constraints::extract_constraints(conn.client()).await {
            Ok(objs) => data.old_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract constraints: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("Old DB: extracting triggers...".into())).await;
        match crate::db::postgres::triggers::extract_triggers(conn.client()).await {
            Ok(objs) => data.old_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract triggers: {}", e)))).await;
                return;
            }
        }

        check_cancel!();
        let _ = tx.send(PullMessage::Status("Old DB: extracting sequences...".into())).await;
        match crate::db::postgres::sequences::extract_sequences(conn.client()).await {
            Ok(objs) => data.old_objects.extend(objs),
            Err(e) => {
                let _ = tx.send(PullMessage::Done(Err(format!("Failed to extract sequences: {}", e)))).await;
                return;
            }
        }

        let _ = tx.send(PullMessage::Status(format!("Old DB: extracted {} objects", data.old_objects.len()))).await;
    }

    check_cancel!();
    let _ = tx.send(PullMessage::Done(Ok(data))).await;
}

fn poll_pull_task(state: &mut State) {
    if let Some(rx) = &mut state.pull_rx {
        // Process all available messages
        loop {
            match rx.try_recv() {
                Ok(PullMessage::Status(status)) => {
                    state.pull_status = status;
                }
                Ok(PullMessage::Done(Ok(data))) => {
                    // Add objects to MemFs - one file per schema
                    let new_base = PathBuf::from("new.database");
                    let old_base = PathBuf::from("old.database");

                    // Clear existing directories in MemFs (so re-pull replaces old data)
                    state.memfs.clear_dir(new_base.clone());
                    state.memfs.clear_dir(old_base.clone());

                    let new_count = data.new_objects.len();
                    let old_count = data.old_objects.len();

                    // Group new objects by schema and write one file per schema
                    let new_grouped = group_by_schema(&data.new_objects);
                    for (schema_name, objects) in new_grouped {
                        let content = generate_schema_file(&schema_name, &objects);
                        let path = new_base.join(format!("{}.sql", schema_name));
                        state.memfs.write(path, content);
                    }

                    // Group old objects by schema and write one file per schema
                    let old_grouped = group_by_schema(&data.old_objects);
                    for (schema_name, objects) in old_grouped {
                        let content = generate_schema_file(&schema_name, &objects);
                        let path = old_base.join(format!("{}.sql", schema_name));
                        state.memfs.write(path, content);
                    }

                    // Auto-generate CLAUDE.md if both databases are pulled
                    if state.has_both_pulled() {
                        let claude_content = generate_claude_md(state);
                        state.memfs.write(PathBuf::from("CLAUDE.md"), claude_content);
                    }

                    state.pull_status = format!(
                        "Done: {} new, {} old objects staged",
                        new_count, old_count
                    );
                    state.pull_in_progress = false;
                    state.pull_rx = None;
                    state.pull_cancel = None;
                    break;
                }
                Ok(PullMessage::Done(Err(e))) => {
                    state.pull_status = e;
                    state.pull_in_progress = false;
                    state.pull_rx = None;
                    state.pull_cancel = None;
                    break;
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    break;
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    state.pull_status = "Pull task disconnected".into();
                    state.pull_in_progress = false;
                    state.pull_rx = None;
                    state.pull_cancel = None;
                    break;
                }
            }
        }
    }
}

fn save_all(app: &App, database_type: DatabaseType, new_db: &mut DbForm, old_db: &mut DbForm, memfs: &MemFs) -> anyhow::Result<()> {
    // Save config
    let new_config = new_db.to_config_partial();
    let old_config = old_db.to_config_partial();

    let config = Config {
        database_type,
        new: Some(new_config.clone()),
        old: Some(old_config.clone()),
    };
    config.save(&app.path)?;

    new_db.saved_config = Some(new_config);
    old_db.saved_config = Some(old_config);

    // Commit MemFs
    memfs.commit()?;

    // Auto-commit to git if git repo exists
    let git_dir = app.path.join(".git");
    if git_dir.exists() && git_dir.is_dir() {
        // Stage all changes
        let _ = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&app.path)
            .output();

        // Commit with auto message
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "pgcmp: auto-commit on save"])
            .current_dir(&app.path)
            .output();
    }

    Ok(())
}

fn ui(f: &mut Frame, state: &mut State) {
    state.click_areas.clear();

    let [tab_bar, content, bottom_bar] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(3),
    ])
    .areas(f.area());

    draw_tab_bar(f, tab_bar, state);
    draw_tab_content(f, content, state);
    draw_bottom_bar(f, bottom_bar, state);

    if state.pull_in_progress {
        draw_pull_dialog(f, f.area(), state);
    }

    if state.migration_dialog_open {
        draw_migration_dialog(f, f.area(), state);
    }

    if state.claude_dialog_open {
        draw_claude_dialog(f, f.area(), state);
    }

    if state.instruct_dialog_open {
        draw_instruct_dialog(f, f.area(), state);
    }

    if let Some(selected) = state.dialog {
        draw_unsaved_dialog(f, f.area(), selected, &mut state.click_areas);
    }
}

fn draw_tab_bar(f: &mut Frame, area: Rect, state: &mut State) {
    // Build tab titles with status indicators
    let titles: Vec<Line> = TABS.iter().map(|t| {
        let (status_char, status_style) = tab_status(*t, state);
        let label = t.label();
        Line::from(vec![
            Span::styled(status_char, status_style),
            Span::raw(label),
        ])
    }).collect();

    let tab_bar_active = matches!(state.focus, Focus::TabBar);
    let border_style = if tab_bar_active {
        Style::new().cyan()
    } else {
        Style::new().dark_gray()
    };

    let tabs = Tabs::new(titles)
        .select(state.tab.index())
        .highlight_style(Style::new().cyan().bold())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(" pgcmp init "),
        );
    f.render_widget(tabs, area);

    // Register click areas for tabs based on actual label positions
    // Tabs widget renders: "Label1 | Label2 | Label3" with divider " | " (3 chars)
    // Now each label has a status char prefix, so add 1 to label length
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let divider_width = 3u16; // " | "

    let mut x = inner.x;
    for tab in TABS.iter() {
        // +1 for status char prefix
        let label_len = tab.label().len() as u16 + 1;
        // Click area width +1 to ensure the last character is fully clickable
        let tab_area = Rect::new(x, inner.y, label_len + 1, inner.height);
        state.click_areas.push((ClickTarget::Tab(*tab), tab_area));
        x += label_len + divider_width;
    }
}

/// Get status indicator for a tab
fn tab_status(tab: Tab, state: &State) -> (&'static str, Style) {
    match tab {
        Tab::NewDb => {
            if state.new_db.is_connected() {
                ("✓", Style::new().green())
            } else if state.new_db.validation_status.contains("required")
                || state.new_db.validation_status.contains("invalid")
                || state.new_db.connection_status.contains("fail")
                || state.new_db.connection_status.contains("Fail") {
                ("✗", Style::new().red())
            } else {
                ("○", Style::new().dark_gray())
            }
        }
        Tab::OldDb => {
            if state.old_db.is_connected() {
                ("✓", Style::new().green())
            } else if state.old_db.validation_status.contains("required")
                || state.old_db.validation_status.contains("invalid")
                || state.old_db.connection_status.contains("fail")
                || state.old_db.connection_status.contains("Fail") {
                ("✗", Style::new().red())
            } else {
                ("○", Style::new().dark_gray())
            }
        }
        Tab::Pull => {
            if state.has_both_pulled() {
                ("✓", Style::new().green())
            } else if state.has_new_pulled() || state.has_old_pulled() {
                ("◐", Style::new().yellow())
            } else {
                ("○", Style::new().dark_gray())
            }
        }
        Tab::Claude => {
            if state.claude_in_memfs() || !state.claude_content().is_empty() {
                ("✓", Style::new().green())
            } else {
                ("○", Style::new().dark_gray())
            }
        }
        Tab::Instruct => {
            if state.instruct_in_memfs() || !state.instruct_content().is_empty() {
                ("✓", Style::new().green())
            } else {
                ("○", Style::new().dark_gray())
            }
        }
        Tab::Migration => {
            // Green if migration exists, gray otherwise
            if !state.migration_content().is_empty() {
                ("✓", Style::new().green())
            } else {
                ("○", Style::new().dark_gray())
            }
        }
        Tab::Setup => {
            if !state.has_git {
                ("✗", Style::new().red())
            } else if state.git_clean {
                ("✓", Style::new().green())
            } else {
                ("!", Style::new().yellow())
            }
        }
    }
}

fn draw_tab_content(f: &mut Frame, area: Rect, state: &mut State) {
    match state.tab {
        Tab::NewDb => draw_db_tab(f, area, state, Tab::NewDb),
        Tab::OldDb => draw_db_tab(f, area, state, Tab::OldDb),
        Tab::Pull => draw_pull_tab(f, area, state),
        Tab::Claude => draw_claude_tab(f, area, state),
        Tab::Instruct => draw_instruct_tab(f, area, state),
        Tab::Migration => draw_migration_tab(f, area, state),
        Tab::Setup => draw_setup_tab(f, area, state),
    }
}

fn draw_db_tab(f: &mut Frame, area: Rect, state: &mut State, tab: Tab) {
    let form = match tab {
        Tab::NewDb => &state.new_db,
        Tab::OldDb => &state.old_db,
        _ => return,
    };

    let (title, description) = match tab {
        Tab::NewDb => (
            "New Database",
            "Database with the new schema to deploy (typically dev)",
        ),
        Tab::OldDb => (
            "Old Database",
            "Database with the current schema (typically production)",
        ),
        _ => return,
    };

    let is_tab_active = state.tab == tab;
    let border_style = if is_tab_active {
        Style::new().cyan()
    } else {
        Style::new().dark_gray()
    };

    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let padding = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );

    let active_field = match state.focus {
        Focus::DbField(f) if is_tab_active => Some(f),
        _ => None,
    };

    // Row 0: Description
    if padding.height > 0 {
        f.render_widget(
            Paragraph::new(description).style(Style::new().dark_gray().italic()),
            Rect::new(padding.x, padding.y, padding.width, 1),
        );
    }

    // Fields
    let fields = [
        (Field::Host, "Host", form.host.value()),
        (Field::Port, "Port", form.port.value()),
        (Field::User, "User", form.user.value()),
        (Field::Password, "Password", form.password.value()),
        (Field::Database, "Database", form.database.value()),
    ];

    for (i, (field, label, value)) in fields.iter().enumerate() {
        let row = i as u16 + 1;
        if padding.y + row >= padding.y + padding.height {
            break;
        }
        let row_area = Rect::new(padding.x, padding.y + row, padding.width, 1);
        let active = active_field == Some(*field);
        draw_field(f, row_area, label, value, active);
        state.click_areas.push((ClickTarget::Field(tab, *field), row_area));
    }

    // TLS row
    let tls_row = 6u16;
    if padding.y + tls_row < padding.y + padding.height {
        let tls_area = Rect::new(padding.x, padding.y + tls_row, padding.width, 1);
        let active = active_field == Some(Field::Tls);
        draw_tls_toggle(f, tls_area, form.tls, active, tab, &mut state.click_areas);
    }

    // Status box
    let status_row = 8u16;
    if padding.y + status_row + 4 <= padding.y + padding.height {
        let status_area = Rect::new(padding.x, padding.y + status_row, padding.width, 5);
        draw_status_box(f, status_area, form, &state.app.path);
    }
}

fn draw_status_box(f: &mut Frame, area: Rect, form: &DbForm, _base_path: &std::path::Path) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().dark_gray());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let validation_style = if form.validation_status.contains("required")
        || form.validation_status.contains("invalid")
    {
        Style::new().red()
    } else if form.validation_status == "Valid" {
        Style::new().green()
    } else {
        Style::new().dark_gray()
    };

    let connection_style = if form.connection_status.contains("fail")
        || form.connection_status.contains("Fail")
    {
        Style::new().red()
    } else if form.connection_status.contains("Connected") {
        Style::new().green()
    } else if form.connection_status.contains("Testing") {
        Style::new().yellow()
    } else {
        Style::new().dark_gray()
    };

    let (saved_text, saved_style) = if form.is_saved() {
        ("Saved", Style::new().green())
    } else {
        ("Unsaved", Style::new().yellow())
    };

    let lines = vec![
        Line::from(vec![
            Span::raw("Validation: ").dark_gray(),
            Span::styled(&form.validation_status, validation_style),
        ]),
        Line::from(vec![
            Span::raw("Connection: ").dark_gray(),
            Span::styled(&form.connection_status, connection_style),
        ]),
        Line::from(vec![
            Span::raw("Saved: ").dark_gray(),
            Span::styled(saved_text, saved_style),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_pull_tab(f: &mut Frame, area: Rect, state: &mut State) {
    let block = Block::default()
        .title(" Pull Schemas ")
        .borders(Borders::ALL)
        .border_style(Style::new().dark_gray());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let padding = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );

    // Description
    if padding.height > 0 {
        f.render_widget(
            Paragraph::new("Pull schema definitions from connected databases into memory.")
                .style(Style::new().dark_gray().italic()),
            Rect::new(padding.x, padding.y, padding.width, 1),
        );
    }

    // Connection status
    if padding.height > 2 {
        let new_status = if state.new_db.is_connected() {
            Span::styled("Connected", Style::new().green())
        } else {
            Span::styled("Not connected", Style::new().red())
        };
        let old_status = if state.old_db.is_connected() {
            Span::styled("Connected", Style::new().green())
        } else {
            Span::styled("Not connected", Style::new().red())
        };

        let status_lines = vec![
            Line::from(vec![Span::raw("New DB: ").dark_gray(), new_status]),
            Line::from(vec![Span::raw("Old DB: ").dark_gray(), old_status]),
        ];
        f.render_widget(
            Paragraph::new(status_lines),
            Rect::new(padding.x, padding.y + 2, padding.width, 2),
        );
    }

    // Pull buttons
    if padding.height > 5 {
        let btn_y = padding.y + 5;
        let active_btn = match state.focus {
            Focus::PullButton(b) => Some(b),
            _ => None,
        };

        // Pull button
        let pull_style = if active_btn == Some(PullButton::Pull) {
            Style::new().cyan().bold().reversed()
        } else {
            Style::new().cyan()
        };
        let pull_label = "[ Pull Database Structure ]";
        let pull_area = Rect::new(padding.x, btn_y, pull_label.len() as u16, 1);
        f.render_widget(Paragraph::new(pull_label).style(pull_style), pull_area);
        state.click_areas.push((ClickTarget::PullButton(PullButton::Pull), pull_area));

        // Clear button
        let clear_style = if active_btn == Some(PullButton::Clear) {
            Style::new().red().bold().reversed()
        } else {
            Style::new().red()
        };
        let clear_label = "[ Clear Database Structure ]";
        let clear_x = padding.x + pull_label.len() as u16 + 2;
        let clear_area = Rect::new(clear_x, btn_y, clear_label.len() as u16, 1);
        f.render_widget(Paragraph::new(clear_label).style(clear_style), clear_area);
        state.click_areas.push((ClickTarget::PullButton(PullButton::Clear), clear_area));
    }

    // Pull status (only show when not pulling - dialog shows status during pull)
    if padding.height > 7 && !state.pull_status.is_empty() && !state.pull_in_progress {
        let status_style = if state.pull_status.contains("failed")
            || state.pull_status.contains("not connected")
            || state.pull_status.contains("Cancelled")
            || state.pull_status.contains("cleared") {
            Style::new().red()
        } else {
            Style::new().green()
        };
        f.render_widget(
            Paragraph::new(state.pull_status.as_str()).style(status_style),
            Rect::new(padding.x, padding.y + 7, padding.width, 1),
        );
    }

    // File counts - on disk vs staged
    if padding.height > 9 {
        // Count files on disk recursively
        fn count_files_recursive(path: &std::path::Path) -> usize {
            let mut count = 0;
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_file() {
                        count += 1;
                    } else if path.is_dir() {
                        count += count_files_recursive(&path);
                    }
                }
            }
            count
        }

        let new_disk_count = count_files_recursive(&state.app.path.join("new.database"));
        let old_disk_count = count_files_recursive(&state.app.path.join("old.database"));

        // Count files staged in memfs
        let memfs_files = state.memfs.list_files();
        let new_staged_count = memfs_files.iter()
            .filter(|(path, is_write)| *is_write && path.starts_with("new.database/"))
            .count();
        let old_staged_count = memfs_files.iter()
            .filter(|(path, is_write)| *is_write && path.starts_with("old.database/"))
            .count();

        let lines = vec![
            Line::from(vec![
                Span::raw("On disk:    ").dark_gray(),
                Span::raw(format!("new.database/ {} files", new_disk_count)).white(),
                Span::raw("  |  ").dark_gray(),
                Span::raw(format!("old.database/ {} files", old_disk_count)).white(),
            ]),
            Line::from(vec![
                Span::raw("Staged:     ").dark_gray(),
                Span::styled(
                    format!("new.database/ {} files", new_staged_count),
                    if new_staged_count > 0 { Style::new().yellow() } else { Style::new().white() }
                ),
                Span::raw("  |  ").dark_gray(),
                Span::styled(
                    format!("old.database/ {} files", old_staged_count),
                    if old_staged_count > 0 { Style::new().yellow() } else { Style::new().white() }
                ),
            ]),
        ];
        f.render_widget(
            Paragraph::new(lines),
            Rect::new(padding.x, padding.y + 9, padding.width, 2),
        );
    }
}

fn draw_claude_tab(f: &mut Frame, area: Rect, state: &mut State) {
    let block = Block::default()
        .title(" Claude ")
        .borders(Borders::ALL)
        .border_style(Style::new().dark_gray());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let padding = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );

    // Get claude content and info
    let content = state.claude_content();
    let size = content.len();
    let line_count = content.lines().count();
    let in_memfs = state.claude_in_memfs();
    let has_both = state.has_both_pulled();

    // Status message
    if padding.height > 0 {
        let status_msg = if has_both {
            Span::styled("Both databases pulled - CLAUDE.md can be generated", Style::new().green())
        } else {
            let mut missing = Vec::new();
            if !state.has_new_pulled() {
                missing.push("new");
            }
            if !state.has_old_pulled() {
                missing.push("old");
            }
            Span::styled(
                format!("Pull {} database(s) first to generate CLAUDE.md", missing.join(" and ")),
                Style::new().yellow(),
            )
        };
        f.render_widget(
            Paragraph::new(Line::from(status_msg)),
            Rect::new(padding.x, padding.y, padding.width, 1),
        );
    }

    // Preview box (first 10 lines)
    let preview_start = 2u16;
    let preview_height = 12u16;
    if padding.height > preview_start + preview_height {
        let preview_area = Rect::new(
            padding.x,
            padding.y + preview_start,
            padding.width.min(80),
            preview_height,
        );

        let save_status = if in_memfs { " (Unsaved)" } else { " (Saved)" };
        let title = format!(" CLAUDE.md - {} bytes, {} lines{} ", size, line_count, save_status);

        let border_style = if in_memfs {
            Style::new().yellow()
        } else {
            Style::new().dark_gray()
        };

        let preview_block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);
        let preview_inner = preview_block.inner(preview_area);
        f.render_widget(preview_block, preview_area);

        let preview_lines: Vec<Line> = content
            .lines()
            .take(10)
            .enumerate()
            .map(|(i, line)| {
                let line_num = i + 1;
                let display_line: String = line.chars().take(preview_inner.width.saturating_sub(6) as usize).collect();
                Line::from(vec![
                    Span::styled(format!("{:4} ", line_num), Style::new().dark_gray()),
                    Span::raw(display_line),
                ])
            })
            .collect();

        if preview_lines.is_empty() {
            let empty_text = "(empty)";
            let center_y = preview_inner.height / 2;
            let center_x = (preview_inner.width.saturating_sub(empty_text.len() as u16)) / 2;
            f.render_widget(
                Paragraph::new(empty_text).style(Style::new().dark_gray().italic()),
                Rect::new(preview_inner.x + center_x, preview_inner.y + center_y, empty_text.len() as u16, 1),
            );
        } else {
            f.render_widget(Paragraph::new(preview_lines).style(Style::new().white()), preview_inner);
        }
    }

    // View Contents button
    let view_btn_y = padding.y + preview_start + preview_height + 1;
    if view_btn_y < padding.y + padding.height {
        let active_btn = match state.focus {
            Focus::ClaudeButton(b) => Some(b),
            _ => None,
        };

        let view_style = if active_btn == Some(ClaudeButton::ViewContents) {
            Style::new().bold().reversed()
        } else {
            Style::new().white()
        };
        let view_area = Rect::new(padding.x, view_btn_y, 18, 1);
        f.render_widget(Paragraph::new("[ View Contents ]").style(view_style), view_area);
        state.click_areas.push((ClickTarget::ClaudeButton(ClaudeButton::ViewContents), view_area));
    }

    // Generate and Revert buttons
    let action_btn_y = padding.y + preview_start + preview_height + 3;
    if action_btn_y < padding.y + padding.height {
        let active_btn = match state.focus {
            Focus::ClaudeButton(b) => Some(b),
            _ => None,
        };

        // Generate button - green if enabled, gray if disabled
        let generate_style = if active_btn == Some(ClaudeButton::Generate) {
            if has_both {
                Style::new().green().bold().reversed()
            } else {
                Style::new().dark_gray().bold().reversed()
            }
        } else if has_both {
            Style::new().green()
        } else {
            Style::new().dark_gray()
        };
        let generate_area = Rect::new(padding.x, action_btn_y, 14, 1);
        f.render_widget(Paragraph::new("[ Generate ]").style(generate_style), generate_area);
        state.click_areas.push((ClickTarget::ClaudeButton(ClaudeButton::Generate), generate_area));

        // Revert button
        let revert_style = if active_btn == Some(ClaudeButton::Revert) {
            Style::new().bold().reversed()
        } else {
            Style::new().white()
        };
        let revert_area = Rect::new(padding.x + 16, action_btn_y, 12, 1);
        f.render_widget(Paragraph::new("[ Revert ]").style(revert_style), revert_area);
        state.click_areas.push((ClickTarget::ClaudeButton(ClaudeButton::Revert), revert_area));
    }
}

fn draw_instruct_tab(f: &mut Frame, area: Rect, state: &mut State) {
    let block = Block::default()
        .title(" Instruct ")
        .borders(Borders::ALL)
        .border_style(Style::new().dark_gray());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let padding = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );

    // Check if textarea content differs from saved content
    let textarea_content = state.instruct_textarea.lines().join("\n");
    let saved_content = state.instruct_content();
    let has_unsaved = textarea_content != saved_content;

    // Status message
    if padding.height > 0 {
        let status_msg = Span::styled(
            "This is extra instructions for Claude",
            Style::new().cyan(),
        );
        f.render_widget(
            Paragraph::new(Line::from(status_msg)),
            Rect::new(padding.x, padding.y, padding.width, 1),
        );
    }

    // Layout: status (1) + gap (1) + textarea (remaining - 2 for buttons) + gap (1) + buttons (1)
    let textarea_start = 2u16;
    let button_height = 1u16;
    let gap = 1u16;
    let textarea_height = padding.height.saturating_sub(textarea_start + button_height + gap);

    if textarea_height > 2 {
        let textarea_area = Rect::new(
            padding.x,
            padding.y + textarea_start,
            padding.width.min(100),
            textarea_height,
        );

        // Configure textarea appearance
        let is_focused = matches!(state.focus, Focus::InstructTextArea);
        let border_style = if is_focused {
            Style::new().cyan()
        } else if has_unsaved {
            Style::new().yellow()
        } else {
            Style::new().dark_gray()
        };

        let title = if has_unsaved {
            " INSTRUCT.md (unsaved) - Ctrl+S to save "
        } else {
            " INSTRUCT.md "
        };

        state.instruct_textarea.set_block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style)
        );
        state.instruct_textarea.set_cursor_line_style(Style::default());
        state.instruct_textarea.set_line_number_style(Style::new().dark_gray());

        f.render_widget(&state.instruct_textarea, textarea_area);
    }

    // Save and Revert buttons
    let btn_y = padding.y + textarea_start + textarea_height + gap;
    if btn_y < padding.y + padding.height {
        let active_btn = match state.focus {
            Focus::InstructButton(b) => Some(b),
            _ => None,
        };

        // Save button - green if there are unsaved changes
        let save_style = if active_btn == Some(InstructButton::Save) {
            if has_unsaved {
                Style::new().green().bold().reversed()
            } else {
                Style::new().dark_gray().bold().reversed()
            }
        } else if has_unsaved {
            Style::new().green()
        } else {
            Style::new().dark_gray()
        };
        let save_area = Rect::new(padding.x, btn_y, 10, 1);
        f.render_widget(Paragraph::new("[ Save ]").style(save_style), save_area);
        state.click_areas.push((ClickTarget::InstructButton(InstructButton::Save), save_area));

        // Revert button
        let revert_style = if active_btn == Some(InstructButton::Revert) {
            Style::new().bold().reversed()
        } else {
            Style::new().white()
        };
        let revert_area = Rect::new(padding.x + 12, btn_y, 12, 1);
        f.render_widget(Paragraph::new("[ Revert ]").style(revert_style), revert_area);
        state.click_areas.push((ClickTarget::InstructButton(InstructButton::Revert), revert_area));
    }
}

fn draw_migration_tab(f: &mut Frame, area: Rect, state: &mut State) {
    let block = Block::default()
        .title(" Migration ")
        .borders(Borders::ALL)
        .border_style(Style::new().dark_gray());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let padding = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );

    // Get migration content and info
    let content = state.migration_content();
    let size = content.len();
    let line_count = content.lines().count();
    let in_memfs = state.migration_in_memfs();

    // Preview box (first 10 lines)
    let preview_start = 1u16;
    let preview_height = 12u16; // 10 lines + 2 for border
    if padding.height > preview_start + preview_height {
        let preview_area = Rect::new(
            padding.x,
            padding.y + preview_start,
            padding.width.min(80),
            preview_height,
        );

        // Build title with file info and save status
        let save_status = if in_memfs { " (Unsaved)" } else { " (Saved)" };
        let title = format!(" MIGRATION.sql - {} bytes, {} lines{} ", size, line_count, save_status);

        let border_style = if in_memfs {
            Style::new().yellow()
        } else {
            Style::new().dark_gray()
        };

        let preview_block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);
        let preview_inner = preview_block.inner(preview_area);
        f.render_widget(preview_block, preview_area);

        // First 10 lines with line numbers
        let preview_lines: Vec<Line> = content
            .lines()
            .take(10)
            .enumerate()
            .map(|(i, line)| {
                let line_num = i + 1;
                // Truncate long lines
                let display_line: String = line.chars().take(preview_inner.width.saturating_sub(6) as usize).collect();
                Line::from(vec![
                    Span::styled(format!("{:4} ", line_num), Style::new().dark_gray()),
                    Span::raw(display_line),
                ])
            })
            .collect();

        if preview_lines.is_empty() {
            // Center "(empty)" in the box
            let empty_text = "(empty)";
            let center_y = preview_inner.height / 2;
            let center_x = (preview_inner.width.saturating_sub(empty_text.len() as u16)) / 2;
            f.render_widget(
                Paragraph::new(empty_text).style(Style::new().dark_gray().italic()),
                Rect::new(preview_inner.x + center_x, preview_inner.y + center_y, empty_text.len() as u16, 1),
            );
        } else {
            f.render_widget(Paragraph::new(preview_lines).style(Style::new().white()), preview_inner);
        }
    }

    // Warning if MIGRATION.sql is empty and staged for save
    let show_warning = content.is_empty() && in_memfs;
    let warning_y = padding.y + preview_start + preview_height + 1;
    if show_warning && warning_y < padding.y + padding.height {
        f.render_widget(
            Paragraph::new("Warning: MIGRATION.sql is empty and will be saved as empty!")
                .style(Style::new().yellow().bold()),
            Rect::new(padding.x, warning_y, padding.width, 1),
        );
    }

    // Offset buttons if warning is shown
    let btn_offset = if show_warning { 2 } else { 0 };

    // View Contents button
    let view_btn_y = padding.y + preview_start + preview_height + 1 + btn_offset;
    if view_btn_y < padding.y + padding.height {
        let active_btn = match state.focus {
            Focus::MigrationButton(b) => Some(b),
            _ => None,
        };

        let view_style = if active_btn == Some(MigrationButton::ViewContents) {
            Style::new().bold().reversed()
        } else {
            Style::new().white()
        };
        let view_area = Rect::new(padding.x, view_btn_y, 18, 1);
        f.render_widget(Paragraph::new("[ View Contents ]").style(view_style), view_area);
        state.click_areas.push((ClickTarget::MigrationButton(MigrationButton::ViewContents), view_area));
    }

    // Clear and Undo Clear buttons
    let action_btn_y = padding.y + preview_start + preview_height + 3 + btn_offset;
    if action_btn_y < padding.y + padding.height {
        let active_btn = match state.focus {
            Focus::MigrationButton(b) => Some(b),
            _ => None,
        };

        // Clear button
        let clear_style = if active_btn == Some(MigrationButton::Clear) {
            Style::new().green().bold().reversed()
        } else {
            Style::new().green()
        };
        let clear_area = Rect::new(padding.x, action_btn_y, 11, 1);
        f.render_widget(Paragraph::new("[ Clear ]").style(clear_style), clear_area);
        state.click_areas.push((ClickTarget::MigrationButton(MigrationButton::Clear), clear_area));

        // Undo Clear button
        let undo_style = if active_btn == Some(MigrationButton::UndoClear) {
            Style::new().bold().reversed()
        } else {
            Style::new().white()
        };
        let undo_area = Rect::new(padding.x + 13, action_btn_y, 16, 1);
        f.render_widget(Paragraph::new("[ Undo Clear ]").style(undo_style), undo_area);
        state.click_areas.push((ClickTarget::MigrationButton(MigrationButton::UndoClear), undo_area));
    }
}

fn draw_migration_dialog(f: &mut Frame, area: Rect, state: &State) {
    let content = state.migration_content();
    let line_count = content.lines().count();

    // Dialog takes most of the screen
    let dialog_width = area.width.saturating_sub(8).min(120);
    let dialog_height = area.height.saturating_sub(6).min(40);
    let dialog_area = center_rect(area, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    // Build scroll indicator
    let visible_height = dialog_height.saturating_sub(4); // account for borders and title
    let max_scroll = (line_count as u16).saturating_sub(visible_height);
    let scroll_info = if max_scroll > 0 {
        format!(
            " (line {}/{}) ",
            state.migration_scroll + 1,
            line_count
        )
    } else {
        String::new()
    };

    let block = Block::default()
        .title(format!(" MIGRATION.sql{} ", scroll_info))
        .title_bottom(" Esc/Enter/q to close | ↑↓/jk scroll | PgUp/PgDn | Home/End ")
        .borders(Borders::ALL)
        .border_style(Style::new().cyan());
    let inner = block.inner(dialog_area);
    f.render_widget(block, dialog_area);

    // Render content with scroll offset
    let visible_lines: Vec<Line> = content
        .lines()
        .skip(state.migration_scroll as usize)
        .take(inner.height as usize)
        .enumerate()
        .map(|(i, line)| {
            let line_num = state.migration_scroll as usize + i + 1;
            Line::from(vec![
                Span::styled(format!("{:4} ", line_num), Style::new().dark_gray()),
                Span::raw(line),
            ])
        })
        .collect();

    let content_widget = if visible_lines.is_empty() && content.is_empty() {
        Paragraph::new("(empty file)").style(Style::new().dark_gray().italic())
    } else {
        Paragraph::new(visible_lines).style(Style::new().white())
    };
    f.render_widget(content_widget, inner);
}

fn draw_claude_dialog(f: &mut Frame, area: Rect, state: &State) {
    let content = state.claude_content();
    let line_count = content.lines().count();

    let dialog_width = area.width.saturating_sub(8).min(120);
    let dialog_height = area.height.saturating_sub(6).min(40);
    let dialog_area = center_rect(area, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let visible_height = dialog_height.saturating_sub(4);
    let max_scroll = (line_count as u16).saturating_sub(visible_height);
    let scroll_info = if max_scroll > 0 {
        format!(
            " (line {}/{}) ",
            state.claude_scroll + 1,
            line_count
        )
    } else {
        String::new()
    };

    let block = Block::default()
        .title(format!(" CLAUDE.md{} ", scroll_info))
        .title_bottom(" Esc/Enter/q to close | ↑↓/jk scroll | PgUp/PgDn | Home/End ")
        .borders(Borders::ALL)
        .border_style(Style::new().cyan());
    let inner = block.inner(dialog_area);
    f.render_widget(block, dialog_area);

    let visible_lines: Vec<Line> = content
        .lines()
        .skip(state.claude_scroll as usize)
        .take(inner.height as usize)
        .enumerate()
        .map(|(i, line)| {
            let line_num = state.claude_scroll as usize + i + 1;
            Line::from(vec![
                Span::styled(format!("{:4} ", line_num), Style::new().dark_gray()),
                Span::raw(line),
            ])
        })
        .collect();

    let content_widget = if visible_lines.is_empty() && content.is_empty() {
        Paragraph::new("(empty file)").style(Style::new().dark_gray().italic())
    } else {
        Paragraph::new(visible_lines).style(Style::new().white())
    };
    f.render_widget(content_widget, inner);
}

fn draw_instruct_dialog(f: &mut Frame, area: Rect, state: &State) {
    let content = state.instruct_content();
    let line_count = content.lines().count();

    let dialog_width = area.width.saturating_sub(8).min(120);
    let dialog_height = area.height.saturating_sub(6).min(40);
    let dialog_area = center_rect(area, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let visible_height = dialog_height.saturating_sub(4);
    let max_scroll = (line_count as u16).saturating_sub(visible_height);
    let scroll_info = if max_scroll > 0 {
        format!(
            " (line {}/{}) ",
            state.instruct_scroll + 1,
            line_count
        )
    } else {
        String::new()
    };

    let block = Block::default()
        .title(format!(" INSTRUCT.md{} ", scroll_info))
        .title_bottom(" Esc/Enter/q to close | ↑↓/jk scroll | PgUp/PgDn | Home/End ")
        .borders(Borders::ALL)
        .border_style(Style::new().cyan());
    let inner = block.inner(dialog_area);
    f.render_widget(block, dialog_area);

    let visible_lines: Vec<Line> = content
        .lines()
        .skip(state.instruct_scroll as usize)
        .take(inner.height as usize)
        .enumerate()
        .map(|(i, line)| {
            let line_num = state.instruct_scroll as usize + i + 1;
            Line::from(vec![
                Span::styled(format!("{:4} ", line_num), Style::new().dark_gray()),
                Span::raw(line),
            ])
        })
        .collect();

    let content_widget = if visible_lines.is_empty() && content.is_empty() {
        Paragraph::new("(empty file)").style(Style::new().dark_gray().italic())
    } else {
        Paragraph::new(visible_lines).style(Style::new().white())
    };
    f.render_widget(content_widget, inner);
}

fn draw_pull_dialog(f: &mut Frame, area: Rect, state: &mut State) {
    let dialog_width = 50u16;
    let dialog_height = 8u16;
    let dialog_area = center_rect(area, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Pulling Database Structure ")
        .borders(Borders::ALL)
        .border_style(Style::new().cyan());
    let inner = block.inner(dialog_area);
    f.render_widget(block, dialog_area);

    // Status message
    let status_style = Style::new().yellow();
    f.render_widget(
        Paragraph::new(state.pull_status.as_str()).style(status_style),
        Rect::new(inner.x + 1, inner.y + 1, inner.width.saturating_sub(2), 2),
    );

    // Cancel button
    let cancel_label = "[ Cancel ]";
    let cancel_width = cancel_label.len() as u16;
    let cancel_x = inner.x + (inner.width.saturating_sub(cancel_width)) / 2;
    let cancel_y = inner.y + inner.height.saturating_sub(2);
    let cancel_area = Rect::new(cancel_x, cancel_y, cancel_width, 1);

    f.render_widget(
        Paragraph::new(cancel_label).style(Style::new().red()),
        cancel_area,
    );
    state.click_areas.push((ClickTarget::PullButton(PullButton::Pull), cancel_area));

    // Help text
    f.render_widget(
        Paragraph::new("Press Esc or C to cancel").style(Style::new().dark_gray()),
        Rect::new(inner.x + 1, inner.y + inner.height.saturating_sub(1), inner.width.saturating_sub(2), 1),
    );
}

fn draw_setup_tab(f: &mut Frame, area: Rect, state: &mut State) {
    let block = Block::default()
        .title(" Setup ")
        .borders(Borders::ALL)
        .border_style(Style::new().dark_gray());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let padding = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );

    let mut y = padding.y;

    // Database Type toggle
    if y < padding.y + padding.height {
        let dbtype_area = Rect::new(padding.x, y, padding.width, 1);
        let active = matches!(state.focus, Focus::SetupDbType);
        draw_dbtype_toggle(f, dbtype_area, state.database_type, active, &mut state.click_areas);
        y += 2;
    }

    // Separator label for Git section
    if y < padding.y + padding.height {
        f.render_widget(
            Paragraph::new("── Git ──").style(Style::new().dark_gray()),
            Rect::new(padding.x, y, padding.width, 1),
        );
        y += 1;
    }

    // Get active button if any
    let active_btn = match state.focus {
        Focus::GitButton(b) => Some(b),
        _ => None,
    };

    if !state.has_git {
        // No git repository - show init button
        if y < padding.y + padding.height {
            f.render_widget(
                Paragraph::new("No git repository found in this directory.")
                    .style(Style::new().yellow()),
                Rect::new(padding.x, y, padding.width, 1),
            );
            y += 2;
        }

        if y < padding.y + padding.height {
            let init_style = if active_btn == Some(GitButton::Init) {
                Style::new().green().bold().reversed()
            } else {
                Style::new().green()
            };
            let init_area = Rect::new(padding.x, y, 14, 1);
            f.render_widget(Paragraph::new("[ Git Init ]").style(init_style), init_area);
            state.click_areas.push((ClickTarget::GitButton(GitButton::Init), init_area));
        }
    } else {
        // Has git repository - show status
        if y < padding.y + padding.height {
            let branch_text = if state.git_branch.is_empty() {
                "Branch: (detached HEAD)".to_string()
            } else {
                format!("Branch: {}", state.git_branch)
            };
            f.render_widget(
                Paragraph::new(branch_text).style(Style::new().cyan().bold()),
                Rect::new(padding.x, y, padding.width, 1),
            );
            y += 2;
        }

        // Status indicator
        if y < padding.y + padding.height {
            let (status_text, status_style) = if state.git_clean {
                ("Status: Clean (no uncommitted changes)", Style::new().green())
            } else {
                ("Status: Dirty (uncommitted changes)", Style::new().yellow())
            };
            f.render_widget(
                Paragraph::new(status_text).style(status_style),
                Rect::new(padding.x, y, padding.width, 1),
            );
            y += 2;
        }

        // Changed files list
        if !state.git_clean && y < padding.y + padding.height {
            f.render_widget(
                Paragraph::new("Changed files:").style(Style::new().bold()),
                Rect::new(padding.x, y, padding.width, 1),
            );
            y += 1;

            // Show up to 10 changed files
            let max_files = 10.min(state.git_status_lines.len());
            for line in state.git_status_lines.iter().take(max_files) {
                if y >= padding.y + padding.height.saturating_sub(4) {
                    break;
                }
                f.render_widget(
                    Paragraph::new(format!("  {}", line)).style(Style::new().dark_gray()),
                    Rect::new(padding.x, y, padding.width, 1),
                );
                y += 1;
            }

            // Show "and X more..." if there are more files
            if state.git_status_lines.len() > max_files {
                if y < padding.y + padding.height.saturating_sub(4) {
                    f.render_widget(
                        Paragraph::new(format!("  ... and {} more", state.git_status_lines.len() - max_files))
                            .style(Style::new().dark_gray().italic()),
                        Rect::new(padding.x, y, padding.width, 1),
                    );
                    y += 1;
                }
            }

            y += 1;
        }

        // Buttons row
        let btn_y = padding.y + padding.height.saturating_sub(2);
        if btn_y > y {
            // Commit All button (only if dirty)
            if !state.git_clean {
                let commit_style = if active_btn == Some(GitButton::CommitAll) {
                    Style::new().green().bold().reversed()
                } else {
                    Style::new().green()
                };
                let commit_area = Rect::new(padding.x, btn_y, 16, 1);
                f.render_widget(Paragraph::new("[ Commit All ]").style(commit_style), commit_area);
                state.click_areas.push((ClickTarget::GitButton(GitButton::CommitAll), commit_area));

                // Refresh button
                let refresh_style = if active_btn == Some(GitButton::Refresh) {
                    Style::new().bold().reversed()
                } else {
                    Style::new().white()
                };
                let refresh_area = Rect::new(padding.x + 18, btn_y, 13, 1);
                f.render_widget(Paragraph::new("[ Refresh ]").style(refresh_style), refresh_area);
                state.click_areas.push((ClickTarget::GitButton(GitButton::Refresh), refresh_area));
            } else {
                // Only refresh button when clean
                let refresh_style = if active_btn == Some(GitButton::Refresh) {
                    Style::new().bold().reversed()
                } else {
                    Style::new().white()
                };
                let refresh_area = Rect::new(padding.x, btn_y, 13, 1);
                f.render_widget(Paragraph::new("[ Refresh ]").style(refresh_style), refresh_area);
                state.click_areas.push((ClickTarget::GitButton(GitButton::Refresh), refresh_area));
            }
        }
    }
}

fn draw_field(f: &mut Frame, area: Rect, label: &str, value: &str, active: bool) {
    let [label_area, value_area] =
        Layout::horizontal([Constraint::Length(10), Constraint::Min(10)]).areas(area);

    f.render_widget(
        Paragraph::new(format!("{}:", label)).style(Style::new().bold()),
        label_area,
    );

    let style = if active {
        Style::new().cyan().add_modifier(Modifier::UNDERLINED)
    } else {
        Style::new().white()
    };

    let text = if active {
        format!("{}_", value)
    } else {
        value.to_string()
    };
    f.render_widget(Paragraph::new(text).style(style), value_area);
}

fn draw_dbtype_toggle(
    f: &mut Frame,
    area: Rect,
    current: DatabaseType,
    active: bool,
    clicks: &mut Vec<(ClickTarget, Rect)>,
) {
    let [label_area, options_area] =
        Layout::horizontal([Constraint::Length(10), Constraint::Min(20)]).areas(area);

    f.render_widget(
        Paragraph::new("Type:").style(Style::new().bold()),
        label_area,
    );

    let style = if active {
        Style::new().cyan()
    } else {
        Style::new().white()
    };

    let mut x_offset = 0u16;
    for db_type in DatabaseType::all() {
        let selected = *db_type == current;
        let label = db_type.display_str();
        let text = if selected {
            format!("● {}", label)
        } else {
            format!("○ {}", label)
        };
        let width = text.len() as u16;
        let option_area = Rect::new(options_area.x + x_offset, options_area.y, width, 1);
        f.render_widget(Paragraph::new(text).style(style), option_area);
        clicks.push((ClickTarget::DbTypeOption(*db_type), option_area));
        x_offset += width + 2;
    }
}

fn draw_tls_toggle(
    f: &mut Frame,
    area: Rect,
    current: TlsMode,
    active: bool,
    tab: Tab,
    clicks: &mut Vec<(ClickTarget, Rect)>,
) {
    let [label_area, options_area] =
        Layout::horizontal([Constraint::Length(10), Constraint::Min(20)]).areas(area);

    f.render_widget(
        Paragraph::new("TLS:").style(Style::new().bold()),
        label_area,
    );

    let no_tls_selected = current == TlsMode::Disable;
    let no_tls_text = if no_tls_selected { "● No TLS" } else { "○ No TLS" };
    let tls_text = if !no_tls_selected { "● TLS" } else { "○ TLS" };

    let style = if active {
        Style::new().cyan()
    } else {
        Style::new().white()
    };

    let no_tls_width = 8u16;
    let tls_width = 5u16;
    let no_tls_area = Rect::new(options_area.x, options_area.y, no_tls_width, 1);
    let tls_area = Rect::new(options_area.x + no_tls_width + 2, options_area.y, tls_width, 1);

    f.render_widget(Paragraph::new(no_tls_text).style(style), no_tls_area);
    f.render_widget(Paragraph::new(tls_text).style(style), tls_area);

    clicks.push((ClickTarget::TlsOption(tab, TlsMode::Disable), no_tls_area));
    clicks.push((ClickTarget::TlsOption(tab, TlsMode::Require), tls_area));
}

fn draw_bottom_bar(f: &mut Frame, area: Rect, state: &mut State) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::new().dark_gray());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [help_area, buttons_area] =
        Layout::horizontal([Constraint::Min(30), Constraint::Length(30)]).areas(inner);

    // Help text
    let help = Line::from(vec![
        "Tab/↑↓".cyan().bold(),
        ": navigate  ".into(),
        "Enter".cyan().bold(),
        ": action  ".into(),
        "Esc".cyan().bold(),
        ": exit".into(),
    ]);
    f.render_widget(
        Paragraph::new(help),
        Rect::new(help_area.x + 1, help_area.y, help_area.width, 1),
    );

    // Save/Cancel buttons
    let active_bottom = match state.focus {
        Focus::BottomButton(b) => Some(b),
        _ => None,
    };

    let save_style = if active_bottom == Some(BottomButton::Save) {
        Style::new().green().bold().reversed()
    } else {
        Style::new().green()
    };
    let cancel_style = if active_bottom == Some(BottomButton::Cancel) {
        Style::new().red().bold().reversed()
    } else {
        Style::new().red()
    };

    let save_label = "[ Save and Close ]";
    let cancel_label = "[ Cancel ]";
    let save_area = Rect::new(buttons_area.x, buttons_area.y, save_label.len() as u16, 1);
    let cancel_area = Rect::new(buttons_area.x + save_label.len() as u16 + 2, buttons_area.y, cancel_label.len() as u16, 1);

    f.render_widget(Paragraph::new(save_label).style(save_style), save_area);
    f.render_widget(Paragraph::new(cancel_label).style(cancel_style), cancel_area);

    state.click_areas.push((ClickTarget::BottomButton(BottomButton::Save), save_area));
    state.click_areas.push((ClickTarget::BottomButton(BottomButton::Cancel), cancel_area));
}

fn draw_unsaved_dialog(
    f: &mut Frame,
    area: Rect,
    selected: DialogButton,
    clicks: &mut Vec<(ClickTarget, Rect)>,
) {
    let dialog_width = 52u16;
    let dialog_height = 7u16;
    let dialog_area = center_rect(area, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Unsaved Changes ")
        .borders(Borders::ALL)
        .border_style(Style::new().yellow());
    let inner = block.inner(dialog_area);
    f.render_widget(block, dialog_area);

    let msg = Paragraph::new("You have unsaved changes. What would you like to do?")
        .style(Style::new().white());
    f.render_widget(
        msg,
        Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), 2),
    );

    let button_y = inner.y + 3;
    let button_row = Rect::new(inner.x, button_y, inner.width, 1);

    let buttons = Layout::horizontal([
        Constraint::Length(16),
        Constraint::Length(12),
        Constraint::Length(12),
    ])
    .flex(Flex::SpaceAround)
    .split(button_row);

    let save_style = if selected == DialogButton::SaveAndClose {
        Style::new().green().bold().reversed()
    } else {
        Style::new().green()
    };
    f.render_widget(Paragraph::new("[ Save & Close ]").style(save_style), buttons[0]);
    clicks.push((ClickTarget::DialogButton(DialogButton::SaveAndClose), buttons[0]));

    let discard_style = if selected == DialogButton::Discard {
        Style::new().red().bold().reversed()
    } else {
        Style::new().red()
    };
    f.render_widget(Paragraph::new("[ Discard ]").style(discard_style), buttons[1]);
    clicks.push((ClickTarget::DialogButton(DialogButton::Discard), buttons[1]));

    let cancel_style = if selected == DialogButton::Cancel {
        Style::new().white().bold().reversed()
    } else {
        Style::new().white()
    };
    f.render_widget(Paragraph::new("[ Cancel ]").style(cancel_style), buttons[2]);
    clicks.push((ClickTarget::DialogButton(DialogButton::Cancel), buttons[2]));
}

/// Generate CLAUDE.md content with instructions and file index
fn generate_claude_md(state: &State) -> String {
    let mut content = String::new();

    // What this file is
    content.push_str("# CLAUDE.md - PostgreSQL Schema Migration Instructions\n\n");

    // Reference to INSTRUCT.md at the top
    content.push_str("**IMPORTANT: Also read `INSTRUCT.md` for additional instructions specific to this project.**\n\n");

    content.push_str("This file provides instructions for Claude to assist with PostgreSQL schema migrations.\n");
    content.push_str("It is auto-generated by `pgcmp init` and should be regenerated after pulling schemas.\n\n");

    // Project files and restrictions
    content.push_str("## Project Files\n\n");
    content.push_str("This project contains the following top-level items:\n\n");
    content.push_str("- `INSTRUCT.md` - Extra instructions for Claude (read this first!)\n");
    content.push_str("- `MIGRATION.sql` - The SQL migration script you will edit\n");
    content.push_str("- `old.database/` - Schema extracted from the OLD database (current production state)\n");
    content.push_str("- `new.database/` - Schema extracted from the NEW database (target state to migrate to)\n\n");

    content.push_str("Each database directory contains one `.sql` file per PostgreSQL schema (e.g., `public.sql`).\n");
    content.push_str("Each schema file contains all objects for that schema: tables, views, functions, indexes, etc.\n\n");

    content.push_str("**IMPORTANT: You MUST NEVER read files other than those listed above.**\n");
    content.push_str("Do not read CONFIG.toml, CLAUDE.md, or any other files. Only read:\n");
    content.push_str("- `INSTRUCT.md` (for extra project-specific instructions)\n");
    content.push_str("- `MIGRATION.sql` (to view/edit the migration script)\n");
    content.push_str("- Files under `old.database/` (to see current schema, e.g., `old.database/public.sql`)\n");
    content.push_str("- Files under `new.database/` (to see target schema, e.g., `new.database/public.sql`)\n\n");

    // Operating loop
    content.push_str("## Operating Loop\n\n");
    content.push_str("**YOU MUST IMPLEMENT THE ACTIONS SPECIFIED IN THE OUTPUT OF `pgcmp test` AS THE PRIMARY SOURCE OF DIRECTION.**\n\n");
    content.push_str("**IMPORTANT: Always use `pgcmp test` to identify differences. Do NOT manually diff the schema files.**\n\n");
    content.push_str("Follow this loop to develop the migration:\n\n");
    content.push_str("1. **Run `pgcmp test`** to see what differences exist between old and new schemas\n");
    content.push_str("2. **Analyze the XML output** to understand what SQL statements are needed\n");
    content.push_str("3. **Edit `MIGRATION.sql`** to add SQL statements that transform old schema to match new\n");
    content.push_str("4. **Run `pgcmp test` again** to verify your changes (runs in a transaction with rollback)\n");
    content.push_str("5. **Repeat until `pgcmp test` shows zero differences** - then the migration is complete\n\n");

    content.push_str("### Key Command\n\n");
    content.push_str("```bash\n");
    content.push_str("pgcmp test    # Test MIGRATION.sql against old database (with rollback)\n");
    content.push_str("```\n\n");

    content.push_str("### MIGRATION.sql Format\n\n");
    content.push_str("**Migration files MUST have this exact structure:**\n\n");
    content.push_str("```sql\n");
    content.push_str("BEGIN TRANSACTION;\n\n");
    content.push_str("-- Your migration SQL statements here\n\n");
    content.push_str("ROLLBACK;\n");
    content.push_str("```\n\n");
    content.push_str("- First statement MUST be `BEGIN TRANSACTION;` (or `BEGIN;`)\n");
    content.push_str("- Last statement MUST be `ROLLBACK;`\n");
    content.push_str("- Do NOT put `COMMIT` anywhere in the file\n");
    content.push_str("- Do NOT put extra `BEGIN` or `ROLLBACK` statements in the middle\n\n");
    content.push_str("This format ensures migrations cannot be accidentally applied. The `pgcmp test`\n");
    content.push_str("command validates this structure before running. To actually apply a migration,\n");
    content.push_str("use `pgcmp apply --commit` which overrides the final ROLLBACK with COMMIT.\n\n");

    content.push_str("**Use DO blocks for procedural logic.**\n\n");
    content.push_str("When you need loops, conditionals, or other procedural logic in your migration,\n");
    content.push_str("use PostgreSQL's anonymous `DO` blocks:\n\n");
    content.push_str("```sql\n");
    content.push_str("DO $$\n");
    content.push_str("BEGIN\n");
    content.push_str("    -- Your procedural code here\n");
    content.push_str("    IF EXISTS (SELECT 1 FROM ...) THEN\n");
    content.push_str("        -- conditional logic\n");
    content.push_str("    END IF;\n");
    content.push_str("END\n");
    content.push_str("$$;\n");
    content.push_str("```\n\n");
    content.push_str("Note: `BEGIN`/`END` inside DO blocks and function bodies are PL/pgSQL block\n");
    content.push_str("delimiters, NOT transaction control - these are perfectly fine to use.\n\n");

    content.push_str("### Understanding the Test Output\n\n");
    content.push_str("The `pgcmp test` command outputs XML showing:\n");
    content.push_str("- **Objects only in NEW**: Need to be created in the migration\n");
    content.push_str("- **Objects only in OLD**: May need to be dropped (or might be intentionally different)\n");
    content.push_str("- **Objects that differ**: Need to be altered to match the new schema\n\n");

    content.push_str("### When to Read Schema Files\n\n");
    content.push_str("The schema files under `old.database/` and `new.database/` are for **context only**.\n");
    content.push_str("You should:\n");
    content.push_str("- **DO** read a schema file when you need full DDL details for a specific object\n");
    content.push_str("- **DO** read schema files to understand table structures, column types, or function bodies\n");
    content.push_str("- **DO NOT** try to manually diff the schema files to find differences\n");
    content.push_str("- **DO NOT** read schema files unless you need specific context for writing migration SQL\n\n");
    content.push_str("Always rely on `pgcmp test` output to identify what needs to change.\n\n");

    content.push_str("### Important: Row Counts\n\n");
    content.push_str("In most cases, the total row count for each table must remain the same after migration.\n");
    content.push_str("If a table was dropped and another similar table was created, a data migration may be\n");
    content.push_str("needed to maintain the row count. If you believe there is a justification for the row\n");
    content.push_str("count being different (e.g., the table is genuinely new or being intentionally removed),\n");
    content.push_str("you MUST explain this to the user before proceeding.\n\n");

    // Schema files index
    content.push_str("## Schema Files\n\n");
    content.push_str("Below is a listing of the schema files extracted from each database.\n");
    content.push_str("Each file contains all objects (tables, views, functions, etc.) for that PostgreSQL schema.\n\n");

    // Collect files from memfs, organized by directory
    let files = state.memfs.list_files();

    // Separate new and old files
    let mut new_files: Vec<&std::path::Path> = Vec::new();
    let mut old_files: Vec<&std::path::Path> = Vec::new();

    for (path, is_write) in &files {
        if !*is_write {
            continue;
        }
        if path.starts_with("new.database/") {
            new_files.push(path);
        } else if path.starts_with("old.database/") {
            old_files.push(path);
        }
    }

    new_files.sort();
    old_files.sort();

    // Write new database files
    content.push_str("### new.database/ (Target State)\n\n");
    if new_files.is_empty() {
        content.push_str("_No objects extracted_\n\n");
    } else {
        content.push_str("```\n");
        for path in &new_files {
            content.push_str(&format!("{}\n", path.display()));
        }
        content.push_str("```\n\n");
    }

    // Write old database files
    content.push_str("### old.database/ (Current State)\n\n");
    if old_files.is_empty() {
        content.push_str("_No objects extracted_\n\n");
    } else {
        content.push_str("```\n");
        for path in &old_files {
            content.push_str(&format!("{}\n", path.display()));
        }
        content.push_str("```\n\n");
    }

    content
}

fn center_rect(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
