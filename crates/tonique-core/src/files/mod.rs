//! New, open and save, the same in every UI: which file the project is
//! in, whether it changed since, the recent projects, asking before
//! changes are lost. Each step returns a [`FileOutcome`]: what the UI has
//! to ask the user (a file to open, where to save, whether to keep
//! unsaved changes) and what to show. The UI only draws the dialogs and
//! answers with [`ProjectFiles::opened`], [`ProjectFiles::saved_to`] or
//! [`ProjectFiles::answer_unsaved`].

pub mod export;

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    config::{config_dir, recent::RecentProjects},
    document::file::EXTENSION,
    document::file::ProjectFile,
    document::file::project_name,
    project::ProjectState,
};

/// What the user asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectAction {
    New,
    /// Choose a project to open.
    Open,
    /// Open this recent project.
    OpenRecent(PathBuf),
    Save,
    SaveAs,
    ClearRecent,
}

/// What waits for the unsaved changes to be saved or let go.
#[derive(Clone, Debug, PartialEq)]
enum AfterPrompt {
    New,
    /// A project to open, or `None` to choose one.
    Open(Option<PathBuf>),
    Close,
}

/// What the UI asks the user next.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// Whether to save the changes first: answer with
    /// [`ProjectFiles::answer_unsaved`].
    Unsaved,
    /// A project to open, from `start_dir`: answer with
    /// [`ProjectFiles::opened`].
    ProjectToOpen { start_dir: PathBuf },
    /// Where to save, from `start_dir` and named `file_name` unless the
    /// user changes it: answer with [`ProjectFiles::saved_to`].
    SavePath {
        start_dir: PathBuf,
        file_name: String,
    },
}

/// What went wrong, shown until dismissed.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub title: String,
    pub lines: Vec<String>,
}

/// What a step left for the UI to do.
#[derive(Clone, Debug, Default, PartialEq)]
#[must_use]
pub struct FileOutcome {
    pub ask: Option<Ask>,
    /// Another project replaced the current one (new or opened): what
    /// referred to its tracks and clips no longer does.
    pub replaced: bool,
    pub notice: Option<Notice>,
    /// Audio files the project opened refers to and weren't found.
    pub missing_files: usize,
    /// The user agreed to close: let the window go.
    pub close: bool,
}

impl FileOutcome {
    fn ask(ask: Ask) -> Self {
        Self {
            ask: Some(ask),
            ..Self::default()
        }
    }

    fn notice(title: String, lines: Vec<String>) -> Self {
        Self {
            notice: Some(Notice { title, lines }),
            ..Self::default()
        }
    }
}

pub struct ProjectFiles {
    /// `None` until first saved or opened.
    path: Option<PathBuf>,
    /// The project as last saved or opened: anything different is unsaved.
    saved: ProjectFile,
    recent: RecentProjects,
    /// What waits for the user's answer about unsaved changes, or for a
    /// file to save them to.
    pending: Option<AfterPrompt>,
    /// The user agreed to close.
    closing: bool,
}

impl ProjectFiles {
    pub fn new(state: &ProjectState) -> Self {
        Self::with_recent(state, RecentProjects::load())
    }

    fn with_recent(state: &ProjectState, recent: RecentProjects) -> Self {
        Self {
            path: None,
            saved: state.project(None),
            recent,
            pending: None,
            closing: false,
        }
    }

    /// The project's file, once saved or opened.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The project's folder, once saved or opened.
    pub fn dir(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }

    /// Recently opened or saved projects, newest first.
    pub fn recent(&self) -> &[PathBuf] {
        self.recent.paths()
    }

    pub fn name(&self) -> String {
        self.path.as_deref().map_or("Untitled".into(), project_name)
    }

    /// Comparing the whole project: not every frame on a large one.
    pub fn is_modified(&self, state: &ProjectState) -> bool {
        state.project(self.dir()) != self.saved
    }

    /// The window's title: the project's name, starred while unsaved.
    pub fn title(&self, state: &ProjectState) -> String {
        let star = if self.is_modified(state) { " *" } else { "" };
        format!("{}{star} — Tonique", self.name())
    }

    /// Whether the window can close now; else the user is asked about the
    /// unsaved changes first, and [`FileOutcome::close`] lets it go.
    pub fn should_close(&mut self, state: &ProjectState) -> bool {
        if self.closing || !self.is_modified(state) {
            return true;
        }
        self.pending = Some(AfterPrompt::Close);
        false
    }

    /// Whether the user is being asked about unsaved changes.
    pub fn asking_unsaved(&self) -> bool {
        self.pending.is_some()
    }

    pub fn request(&mut self, action: ProjectAction, state: &mut ProjectState) -> FileOutcome {
        match action {
            ProjectAction::New => self.after_prompt(AfterPrompt::New, state),
            ProjectAction::Open => self.after_prompt(AfterPrompt::Open(None), state),
            ProjectAction::OpenRecent(path) => {
                self.after_prompt(AfterPrompt::Open(Some(path)), state)
            }
            ProjectAction::Save => self.save(state),
            ProjectAction::SaveAs => self.save_as(),
            ProjectAction::ClearRecent => {
                self.recent.clear();
                self.recent.save();
                FileOutcome::default()
            }
        }
    }

    /// The answer about unsaved changes: save them first (`Some(true)`),
    /// let them go (`Some(false)`), or cancel (`None`).
    pub fn answer_unsaved(&mut self, save: Option<bool>, state: &mut ProjectState) -> FileOutcome {
        match save {
            None => {
                self.pending = None;
                FileOutcome::default()
            }
            Some(false) => match self.pending.take() {
                Some(then) => self.run(then, state),
                None => FileOutcome::default(),
            },
            // What waits runs once saved.
            Some(true) => self.save(state),
        }
    }

    /// The project the user picked to open, or `None` if they didn't.
    pub fn opened(&mut self, path: Option<PathBuf>, state: &mut ProjectState) -> FileOutcome {
        match path {
            Some(path) => self.open(&path, state),
            None => FileOutcome::default(),
        }
    }

    /// Where the user chose to save, or `None` if they didn't: what
    /// waited for the save is let go then.
    pub fn saved_to(&mut self, path: Option<PathBuf>, state: &mut ProjectState) -> FileOutcome {
        let Some(mut path) = path else {
            self.pending = None;
            return FileOutcome::default();
        };
        if path
            .extension()
            .is_none_or(|extension| extension != EXTENSION)
        {
            path.set_extension(EXTENSION);
        }
        self.write(&path, state)
    }

    /// Run `then` now, or once the user answered about unsaved changes.
    fn after_prompt(&mut self, then: AfterPrompt, state: &mut ProjectState) -> FileOutcome {
        if self.is_modified(state) {
            self.pending = Some(then);
            return FileOutcome::ask(Ask::Unsaved);
        }
        self.run(then, state)
    }

    fn run(&mut self, then: AfterPrompt, state: &mut ProjectState) -> FileOutcome {
        match then {
            AfterPrompt::New => {
                state.new_project();
                self.path = None;
                self.saved = state.project(None);
                FileOutcome {
                    replaced: true,
                    ..FileOutcome::default()
                }
            }
            AfterPrompt::Open(Some(path)) => self.open(&path, state),
            AfterPrompt::Open(None) => FileOutcome::ask(Ask::ProjectToOpen {
                start_dir: self.start_dir(),
            }),
            AfterPrompt::Close => {
                self.closing = true;
                FileOutcome {
                    close: true,
                    ..FileOutcome::default()
                }
            }
        }
    }

    fn open(&mut self, path: &Path, state: &mut ProjectState) -> FileOutcome {
        let project = match ProjectFile::read(path) {
            Ok(project) => project,
            Err(error) => {
                if !path.exists() {
                    self.recent.remove(path);
                    self.recent.save();
                }
                let title = format!("Couldn't open “{}”", project_name(path));
                return FileOutcome::notice(title, vec![error]);
            }
        };
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let problems = state.load_project(&project, &dir);
        self.saved = state.project(Some(&dir));
        self.remember(path);
        self.path = Some(path.to_path_buf());
        let mut outcome = if problems.is_empty() {
            FileOutcome::default()
        } else {
            let title = format!("“{}” opened with problems", project_name(path));
            FileOutcome::notice(title, problems)
        };
        outcome.replaced = true;
        outcome.missing_files = state.missing_files().len();
        outcome
    }

    /// Save to the project's file, or ask where.
    fn save(&mut self, state: &mut ProjectState) -> FileOutcome {
        match self.path.clone() {
            Some(path) => self.write(&path, state),
            None => self.save_as(),
        }
    }

    fn save_as(&self) -> FileOutcome {
        FileOutcome::ask(Ask::SavePath {
            start_dir: self.start_dir(),
            file_name: format!("{}.{EXTENSION}", self.name()),
        })
    }

    /// Writes the project to `path`, then runs what waited for it.
    fn write(&mut self, path: &Path, state: &mut ProjectState) -> FileOutcome {
        let project = state.project(path.parent());
        if let Err(error) = project.write(path) {
            // What waited stays put: the changes aren't saved.
            self.pending = None;
            let title = format!("Couldn't save “{}”", project_name(path));
            return FileOutcome::notice(title, vec![error]);
        }
        self.remember(path);
        self.path = Some(path.to_path_buf());
        self.saved = project;
        match self.pending.take() {
            Some(then) => self.run(then, state),
            None => FileOutcome::default(),
        }
    }

    fn remember(&mut self, path: &Path) {
        self.recent.add(path);
        self.recent.save();
    }

    /// Where file pickers start: the project's folder, else the home
    /// folder.
    fn start_dir(&self) -> PathBuf {
        self.dir()
            .map(Path::to_path_buf)
            .or_else(std::env::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// Save unsaved changes to a new file after a crash: next to the
    /// project, or with the settings if it was never saved or that folder
    /// can't be written. Returns where, or `None` if nothing was unsaved.
    pub fn save_recovery(&self, state: &ProjectState) -> Result<Option<PathBuf>, String> {
        if !self.is_modified(state) {
            return Ok(None);
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let file = format!("{} (recovered {stamp}).{EXTENSION}", self.name());
        let dirs = self
            .dir()
            .map(Path::to_path_buf)
            .into_iter()
            .chain(config_dir().map(|dir| dir.join("recovery")));
        let mut errors = Vec::new();
        for dir in dirs {
            let path = dir.join(&file);
            let written = fs::create_dir_all(&dir)
                .map_err(|error| error.to_string())
                .and_then(|()| state.project(Some(&dir)).write(&path));
            match written {
                Ok(()) => return Ok(Some(path)),
                Err(error) => errors.push(format!("{}: {error}", dir.display())),
            }
        }
        if errors.is_empty() {
            errors.push("no folder to save it in".into());
        }
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonique_engine::engine::{Engine, EngineConfig};

    fn state() -> ProjectState {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        ProjectState::new(engine)
    }

    /// A folder of its own for a test, empty.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tonique-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a temporary folder");
        dir
    }

    fn files(state: &ProjectState) -> ProjectFiles {
        ProjectFiles::with_recent(state, RecentProjects::default())
    }

    #[test]
    fn unsaved_changes_are_asked_about_then_saved_before_opening() {
        let dir = folder("files-open");
        let mut state = state();
        let mut files = files(&state);
        let other = dir.join(format!("other.{EXTENSION}"));
        state.project(Some(&dir)).write(&other).expect("written");

        state.add_track();
        let outcome = files.request(ProjectAction::OpenRecent(other.clone()), &mut state);
        assert_eq!(outcome.ask, Some(Ask::Unsaved));
        // Saving asks where: never saved.
        let outcome = files.answer_unsaved(Some(true), &mut state);
        assert!(matches!(outcome.ask, Some(Ask::SavePath { .. })));
        // Saved without the extension typed: then the other one opens.
        let outcome = files.saved_to(Some(dir.join("song")), &mut state);
        assert!(outcome.replaced);
        assert!(dir.join(format!("song.{EXTENSION}")).exists());
        assert_eq!(files.path(), Some(other.as_path()));
        assert_eq!(state.track_count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancelling_the_save_keeps_the_project() {
        let mut state = state();
        let mut files = files(&state);
        state.add_track();
        let outcome = files.request(ProjectAction::New, &mut state);
        assert_eq!(outcome.ask, Some(Ask::Unsaved));
        let _ = files.answer_unsaved(Some(true), &mut state);
        assert_eq!(files.saved_to(None, &mut state), FileOutcome::default());
        assert!(!files.asking_unsaved());
        assert_eq!(state.track_count(), 1, "not replaced");
    }

    #[test]
    fn closing_waits_for_the_answer() {
        let mut state = state();
        let mut files = files(&state);
        assert!(files.should_close(&state), "nothing unsaved");
        state.add_track();
        assert!(!files.should_close(&state));
        assert!(files.asking_unsaved());
        let outcome = files.answer_unsaved(Some(false), &mut state);
        assert!(outcome.close);
        assert!(files.should_close(&state), "let through once agreed");
    }

    #[test]
    fn a_missing_project_is_forgotten() {
        let dir = folder("files-missing");
        let mut state = state();
        let mut files = files(&state);
        let gone = dir.join(format!("gone.{EXTENSION}"));
        files.recent.add(&gone);
        let outcome = files.request(ProjectAction::OpenRecent(gone), &mut state);
        assert!(outcome.notice.is_some());
        assert!(!outcome.replaced);
        assert!(files.recent().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// After a crash, unsaved changes land in a new file next to the
    /// project; nothing is written when there were none.
    #[test]
    fn recovery_saves_unsaved_changes_next_to_the_project() {
        let dir = folder("recovery");
        let mut state = state();
        let mut files = files(&state);
        files.path = Some(dir.join(format!("song.{EXTENSION}")));
        assert_eq!(files.save_recovery(&state), Ok(None));

        state.add_track();
        let path = files.save_recovery(&state).ok().flatten().expect("saved");
        assert_eq!(path.parent(), Some(dir.as_path()));
        assert!(path.to_string_lossy().contains("song (recovered"));
        let recovered = ProjectFile::read(&path).expect("read back");
        assert_eq!(recovered, state.project(Some(&dir)));
        let _ = fs::remove_dir_all(&dir);
    }
}
