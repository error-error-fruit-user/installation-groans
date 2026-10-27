//! Where an install goes: its [`Places`], and the [`Location`] it installs in.

use std::path::{Path, PathBuf};

use crate::{
    Installer, Kind, Mode, Outcome, Release, Screen,
    installation::Scratch,
    journal, note,
    screen::{self, STATE},
    warn,
};

/// Where an install goes, as its flags say.
#[derive(Clone, Debug)]
pub struct Places {
    /// In place, or for you.
    pub mode: Mode,
    /// The folder this program is in (an AppImage's own, not its mount).
    pub here: PathBuf,
    /// The folder the payload goes in.
    pub dir: PathBuf,
    /// Where everything else goes: downloads, `install.log`, `install.state`.
    pub artifacts: PathBuf,
    /// `--reinstall`: does everything again.
    pub reinstall: bool,
}

impl Places {
    /// Where `cli` says the install goes.
    pub fn of<C: Installer>(cli: &C) -> Outcome<Places> {
        let here = program_dir().ok_or("couldn't tell where this program is")?;
        let dir = match &cli.dir {
            Some(dir) => std::path::absolute(dir).unwrap_or_else(|_| dir.clone()),
            None => install_dir::<C>(cli.mode, &here)?,
        };
        if cli.mode == Mode::InPlace && !dir.is_dir() {
            return Err(format!("{} isn't a folder", dir.display()));
        }
        let artifacts = match &cli.artifacts {
            Some(given) => std::path::absolute(given).unwrap_or_else(|_| given.clone()),
            None => artifacts_dir(&dir, &here),
        };
        Ok(Places {
            mode: cli.mode,
            here,
            dir,
            artifacts,
            reinstall: cli.reinstall,
        })
    }

    /// Where the command goes for `--mode user`, as it is shown.
    pub fn bin(&self) -> PathBuf {
        match std::env::var_os("HOME") {
            Some(home) if !cfg!(windows) => Path::new(&home).join(".local").join("bin"),
            _ => self.dir.clone(),
        }
    }
}

/// Where the installer whose command line is `C` installs: its places, and its releases there
/// (`location.releases.tdl`). Named by [`macro@crate::location`].
pub struct Location<C: Installer> {
    pub places: Places,
    pub releases: C::Releases,
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
            places,
        }
    }

    /// `name` in the artifacts (`location.artifact("setup.toml")`).
    pub fn artifact(&self, name: impl AsRef<Path>) -> PathBuf {
        self.artifacts.join(name)
    }

    /// Everything `cli` asks for: its releases put in place (and on your PATH for `--mode user`),
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
        journal::open(&self.artifacts, "install.log");
        note!("this program is in {}", self.here.display());
        note!(
            "mode {:?}, installing in {}, artifacts in {}",
            self.mode,
            self.dir.display(),
            self.artifacts.display()
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
        let what = if C::NAME.is_empty() { C::ABOUT } else { C::NAME };
        screen::began(what);
        journal::tell(
            "begin",
            &[("what", what), ("place", &self.dir.display().to_string())],
            &format!("Installing {what} in {}", self.dir.display()),
        );
        let mut ok = true;
        let releases: Vec<_> = parts
            .iter()
            .filter(|part| part.wanted())
            .filter_map(|part| match part.kind {
                Kind::Release(installation, name) => {
                    Some(Release::new(name, installation, &self.places, part.on))
                }
                _ => None,
            })
            .collect();
        let scratch = Scratch::new();
        if !releases.is_empty() {
            journal::section("install");
        }
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
                Kind::Artifact(installation) => Some((part.flag, installation)),
                _ => None,
            })
            .collect();
        if !artifacts.is_empty() {
            journal::section("download");
        }
        for (flag, installation) in artifacts {
            let name = installation.called(flag);
            match installation.install(flag, &self.artifacts, &scratch, self.reinstall) {
                Ok(Some(path)) => note!("{name}: {}", path.display()),
                Ok(None) => {}
                Err(problem) => {
                    warn!("{name} wasn't installed: {problem}");
                    ok = false;
                }
            }
        }
        if let Some(release) = releases.first() {
            journal::shortcut(
                release.name,
                release.name,
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

/// The plan: what saying yes does, step by step, under the warning; with the flags that change it
/// at the end when the question is groans.rs' (`flags`).
pub(crate) fn plan<C: Installer>(cli: &C, places: &Places, flags: bool) -> Vec<String> {
    let name = if C::NAME.is_empty() { "it" } else { C::NAME };
    let mut lines = vec!["Saying yes:".to_string()];
    lines.push(match places.mode {
        Mode::InPlace => format!("  - installs in place, in {}, and adds no command", places.dir.display()),
        Mode::User => format!(
            "  - installs into {}, with the {name} command on your PATH ({})",
            places.dir.display(),
            places.bin().display()
        ),
    });
    lines.push(format!(
        "  - puts everything else it makes, its artifacts, in {}: its log among them",
        places.artifacts.display()
    ));
    for part in cli.parts().iter().filter(|part| part.wanted()) {
        lines.push(match part.kind {
            Kind::Artifact(installation) => installation.plan(part.doc, part.flag, &places.artifacts),
            _ => format!("  - {}", part.doc),
        });
    }
    if cli.desktop_shortcut {
        lines.push(format!("  - makes a shortcut to {name} on your desktop, with the system's lock icon"));
    }
    if cli.menu_entry {
        lines.push(format!("  - makes an entry for {name} in your app menu, with the system's lock icon"));
    }
    if cli.run {
        lines.push(format!("  - runs {name} once it is done"));
    }
    lines.push("  - leaves alone what is there already".into());
    let steps = journal::Steps::open(&places.artifacts, cli.reinstall);
    let earlier = steps.earlier();
    if !earlier.is_empty() {
        lines.push(format!(
            "  - skips what an earlier run finished ({}), picking up where it left off (--reinstall does it all again)",
            earlier.join(", ")
        ));
    }
    if flags {
        lines.push(String::new());
        lines.push(format!(
            "To change that, here or after the y: {} (--help says what each does).",
            crate::command_line::flag_names(cli).join(" ")
        ));
    }
    lines
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

/// The folder the payload is installed to in `mode`; `here` is where this program is.
fn install_dir<C: Installer>(mode: Mode, here: &Path) -> Outcome<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| format!("${name} isn't set; pass --dir"))
    };
    let name = if C::NAME.is_empty() { env!("CARGO_PKG_NAME") } else { C::NAME };
    Ok(match (mode, cfg!(windows)) {
        (Mode::InPlace, _) => here.to_path_buf(),
        (Mode::User, true) => var("LOCALAPPDATA")?.join("Programs").join(name),
        (Mode::User, false) => var("HOME")?.join(".local").join("opt").join(name),
    })
}

/// Where everything but the payload goes: when this runs in place from a project's
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
