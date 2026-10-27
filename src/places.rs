//! Where an install goes: its [`Places`], and the [`Location`] it installs in.

use std::path::{Path, PathBuf};

use crate::{
    Artifact, Installer, Kind, Mode, Outcome, Release, Screen,
    installation::Scratch,
    journal, note,
    screen::{self, STATE},
    warn,
};

/// What [`Places::of`] says until the install is told where.
pub(crate) const NO_MODE: &str = "Nothing was done: say where to install it, with --locally or --user-wide";

/// Where an install goes, as its flags say.
#[derive(Clone, Debug)]
pub struct Places {
    /// In place, or for you.
    pub mode: Mode,
    /// The folder the release it installs is in: this program's (an AppImage's own, not its
    /// mount), or `--from`.
    pub here: PathBuf,
    /// The folder the payload goes in.
    pub dir: PathBuf,
    /// Where everything else goes: downloads, `install.log`, `install.state`.
    pub artifacts: PathBuf,
    /// `--reinstall`: does everything again.
    pub reinstall: bool,
}

impl Places {
    /// Where `cli` says the install goes; [`NO_MODE`] until it says where.
    pub fn of<C: Installer>(cli: &C) -> Outcome<Places> {
        let here = here(cli)?;
        let mode = cli.mode.ok_or(NO_MODE)?;
        let dir = match &cli.dir {
            Some(dir) => std::path::absolute(dir).unwrap_or_else(|_| dir.clone()),
            None => install_dir::<C>(mode, &here)?,
        };
        if mode == Mode::InPlace && !dir.is_dir() {
            return Err(format!("{} isn't a folder", dir.display()));
        }
        let artifacts = match &cli.artifacts {
            Some(given) => std::path::absolute(given).unwrap_or_else(|_| given.clone()),
            None => artifacts_dir(&dir, &here),
        };
        Ok(Places {
            mode,
            here,
            dir,
            artifacts,
            reinstall: cli.reinstall,
        })
    }

    /// Where the command goes for `--user-wide`.
    pub fn bin(&self) -> PathBuf {
        match std::env::var_os("HOME") {
            Some(home) if !cfg!(windows) => Path::new(&home).join(".local").join("bin"),
            _ => self.dir.clone(),
        }
    }
}

/// Where the installer whose command line is `C` installs: its places (through `Deref`:
/// `location.dir`, `location.mode`), its releases there (`location.releases.tdl`) and its artifacts
/// (`location.artifacts.uzdoom`, and the artifacts' folder itself through `Deref`). Named by
/// [`macro@crate::location`].
pub struct Location<C: Installer> {
    pub places: Places,
    pub releases: C::Releases,
    pub artifacts: C::Artifacts,
}

impl<C: Installer> std::ops::Deref for Location<C> {
    type Target = Places;
    fn deref(&self) -> &Places {
        &self.places
    }
}

impl<C: Installer> Location<C> {
    pub(crate) fn new(cli: &C, places: Places) -> Self {
        Location {
            releases: cli.releases(&places),
            artifacts: cli.artifacts(&places),
            places,
        }
    }

    /// `name` in the artifacts (`location.artifact("setup.toml")`).
    pub fn artifact(&self, name: impl AsRef<Path>) -> PathBuf {
        self.places.artifacts.join(name)
    }

    /// Everything `cli` asks for: its releases put in place (and on your PATH for `--user-wide`),
    /// its artifacts downloaded, its shortcuts made, said in the voice and written in
    /// `install.log` in the artifacts. The screen gives way to the journal, whose lines stay in the
    /// terminal after. Returns whether all of it went well.
    pub fn install(&self, terminal: &mut Screen, cli: &C) -> bool {
        terminal.leave();
        journal::speak_in(
            crate::TOILING_THROUGH_HELL,
            crate::COMPUTER_SPEAK,
            cli.computer_speak,
        );
        let places = &self.places;
        let made = [&places.dir, &places.artifacts]
            .into_iter()
            .try_for_each(|folder| {
                std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))
            });
        journal::open(&places.artifacts, "install.log");
        note!("installing the release in {}", places.here.display());
        note!(
            "mode {:?}, installing in {}, artifacts in {}",
            places.mode,
            places.dir.display(),
            places.artifacts.display()
        );
        let parts = cli.parts();
        note!(
            "parts: {}",
            parts
                .iter()
                .map(|part| format!("{} {}", part.flag, part.wanted()))
                .collect::<Vec<_>>()
                .join(", ")
        );
        screen::began(C::TITLE);
        journal::tell(
            "begin",
            &[("what", C::TITLE), ("place", &places.dir.display().to_string())],
            &format!("Installing {} in {}", C::TITLE, places.dir.display()),
        );
        if let Err(problem) = made {
            screen::fail(&problem);
            return false;
        }
        let mut ok = true;
        let releases: Vec<_> = parts
            .iter()
            .filter(|part| part.wanted())
            .filter_map(|part| match part.kind {
                Kind::Release(installation, name) => {
                    Some(Release::new(name, installation, places, part.on))
                }
                _ => None,
            })
            .collect();
        let scratch = Scratch::new();
        for release in &releases {
            if let Err(problem) = release.install(&scratch) {
                warn!("{} wasn't installed: {problem}", release.name);
                ok = false;
            }
        }
        let artifacts: Vec<_> = parts
            .iter()
            .filter(|part| part.wanted())
            .filter_map(|part| match part.kind {
                Kind::Artifact(installation) => {
                    Some(Artifact::new(part.flag, installation, places, part.on))
                }
                _ => None,
            })
            .collect();
        if !artifacts.is_empty() {
            journal::section("download");
        }
        for artifact in artifacts {
            match artifact.install(&scratch, places.reinstall) {
                Ok(Some(path)) => note!("{}: {}", artifact.name, path.display()),
                Ok(None) => {}
                Err(problem) => {
                    warn!("{} wasn't installed: {problem}", artifact.name);
                    ok = false;
                }
            }
        }
        if let Some(release) = releases.first() {
            journal::shortcut(
                C::TITLE,
                C::FOLDER,
                &[release.path.display().to_string()],
                C::ABOUT,
                cli.desktop_shortcut,
                cli.menu_entry,
            );
        }
        if !ok && let Ok(mut state) = STATE.lock() {
            state.failed = true;
        }
        ok
    }
}

/// The plan: what saying yes does, step by step, under the warning (until it is told where, that
/// it must be); with the flags that change it at the end when the question is the journal's
/// (`flags`).
pub(crate) fn plan<C: Installer>(cli: &C, flags: bool) -> Vec<String> {
    let title = C::TITLE;
    let places = match Places::of(cli) {
        Ok(places) => places,
        Err(problem) if problem == NO_MODE => {
            let local = here(cli)
                .map(|here| here.display().to_string())
                .unwrap_or_else(|problem| problem);
            let user = here(cli)
                .and_then(|here| install_dir::<C>(Mode::User, &here))
                .map(|dir| dir.display().to_string())
                .unwrap_or_else(|problem| problem);
            return vec![
                format!("Say where to install {title}, {}:", if flags { "here after the y or on the command line" } else { "here or on the command line" }),
                format!("  --locally    in place, in {local}"),
                format!("  --user-wide  for you only, in {user}"),
                "One of them is needed: without it, nothing is done.".into(),
            ];
        }
        Err(problem) => return vec![format!("It can't go there: {problem}")],
    };
    let mut lines = vec!["Saying yes will:".to_string()];
    lines.push(match places.mode {
        Mode::InPlace => format!("  - install {title} in place, in {}, adding no command", places.dir.display()),
        Mode::User => format!(
            "  - install {title} for you only, in {}{}",
            places.dir.display(),
            if cli.parts().iter().any(|part| matches!(part.kind, Kind::Release(..))) {
                format!(", its command on your PATH ({})", places.bin().display())
            } else {
                String::new()
            }
        ),
    });
    lines.push(format!(
        "  - put everything else it makes, its artifacts, in {}: its log among them",
        places.artifacts.display()
    ));
    lines.extend(cli.plan(&places));
    if cli.desktop_shortcut {
        lines.push(format!("  - make a shortcut to {title} on your desktop, with the system's lock icon"));
    }
    if cli.menu_entry {
        lines.push(format!("  - make an entry for {title} in your app menu, with the system's lock icon"));
    }
    lines.push("  - leave alone what is there already".into());
    let steps = journal::Steps::open(&places.artifacts, cli.reinstall);
    // (a step marked with what it was done with, `tdl <mark>`, by its name alone, each once)
    let mut earlier: Vec<&str> = Vec::new();
    for step in steps.earlier() {
        let name = step.split([' ', ':']).next().unwrap_or(step);
        if !earlier.contains(&name) {
            earlier.push(name);
        }
    }
    if !earlier.is_empty() {
        lines.push(format!(
            "  - skip what an earlier run finished ({}), picking up where it left off (--reinstall does it all again)",
            earlier.join(", ")
        ));
    }
    if flags {
        lines.push(String::new());
        lines.push(format!(
            "To change that, here or after the y: {} (--help says what each does).",
            crate::command_line::changing(cli)
        ));
    }
    lines
}

/// The folder the release it installs is in: `--from`, or this program's.
fn here<C: Installer>(cli: &C) -> Outcome<PathBuf> {
    match &cli.from {
        Some(from) => Ok(from
            .canonicalize()
            .or_else(|_| std::path::absolute(from))
            .unwrap_or_else(|_| from.clone())),
        None => program_dir().ok_or_else(|| "couldn't tell where this program is".into()),
    }
}

/// The folder this program is in. An AppImage runs from a mount that goes away with it, so for
/// one it is the folder of the AppImage file itself (`$APPIMAGE`).
pub(crate) fn program_dir() -> Option<PathBuf> {
    let exe = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| std::env::current_exe().ok())?;
    exe.parent().map(Path::to_path_buf)
}

/// The folder the payload is installed to in `mode`; `here` is where the release is.
fn install_dir<C: Installer>(mode: Mode, here: &Path) -> Outcome<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| format!("${name} isn't set; pass --dir"))
    };
    Ok(match (mode, cfg!(windows)) {
        (Mode::InPlace, _) => here.to_path_buf(),
        (Mode::User, true) => var("LOCALAPPDATA")?.join("Programs").join(C::FOLDER),
        (Mode::User, false) => var("HOME")?.join(".local").join("opt").join(C::FOLDER),
    })
}

/// Where everything but the payload goes: when this installs in place from a project's
/// `published/releases/`, that project's `published/artifacts/`, or else `<dir>/artifacts/`.
fn artifacts_dir(dir: &Path, here: &Path) -> PathBuf {
    match here.parent() {
        Some(published)
            if dir == here
                && here.file_name().is_some_and(|name| name == "releases")
                && published.file_name().is_some_and(|name| name == "published") =>
        {
            published.join("artifacts")
        }
        _ => dir.join("artifacts"),
    }
}
