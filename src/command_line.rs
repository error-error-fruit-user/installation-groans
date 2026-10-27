//! The command line every installer has: [`Flags`], and its own parts from [`macro@crate::cli`].

use std::{ops::DerefMut, path::PathBuf};

use crate::{Artifact, Installation, Outcome, Places};

/// Where an install puts its payload: never a default, `--locally` or `--user-wide`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// `--locally`: where the installer is (or `--from`, or `--dir`); nothing is copied elsewhere
    /// and no command is added.
    InPlace,
    /// `--user-wide`: copied into your own folder (`~/.local/opt/<folder>`,
    /// `%LOCALAPPDATA%\Programs\<folder>`), with its command on your PATH.
    User,
}

impl Mode {
    /// Its flag, for an installer that runs another (`--locally`).
    pub fn flag(self) -> &'static str {
        match self {
            Mode::InPlace => "--locally",
            Mode::User => "--user-wide",
        }
    }
}

/// The flags every install has, read through the installer's struct (`cli.reinstall`).
#[derive(Clone, Debug, Default)]
pub struct Flags {
    /// `--accept`: goes ahead without asking, with the defaults for what isn't given; where is
    /// still needed.
    pub accept: bool,
    /// `--locally` or `--user-wide`; without either, nothing is done.
    pub mode: Option<Mode>,
    /// `--dir DIR`: the folder the payload goes in.
    pub dir: Option<PathBuf>,
    /// `--artifacts DIR`: where everything else goes.
    pub artifacts: Option<PathBuf>,
    /// `--from DIR`: installs the release in DIR as if this ran from there.
    pub from: Option<PathBuf>,
    /// `--reinstall`: does everything again, where a run picks up where the last left off.
    pub reinstall: bool,
    /// `--desktop-shortcut`.
    pub desktop_shortcut: bool,
    /// `--menu-entry`.
    pub menu_entry: bool,
    /// `--computer-speak`: speaks plainly, rather than as a descent through hell.
    pub computer_speak: bool,
}

/// The flags of [`Flags`] that are on or off, with their help, in the order they are shown.
const SWITCHES: [(&str, &str); 5] = [
    ("accept", "Goes ahead without asking, with the defaults for what isn't given (where is still needed)"),
    ("reinstall", "Does everything again, where a run picks up where the last one left off"),
    ("desktop-shortcut", "Makes a shortcut on the desktop, with the system's lock icon"),
    ("menu-entry", "Makes an entry in the app menu (the Start menu on Windows), with the system's lock icon"),
    ("computer-speak", "Says what it does plainly and formally, rather than as a descent through hell"),
];

impl Flags {
    /// The switch named `flag` (as on the command line, `menu-entry`), if there is one.
    pub fn switch(&mut self, flag: &str) -> Option<&mut bool> {
        Some(match flag {
            "accept" => &mut self.accept,
            "reinstall" => &mut self.reinstall,
            "desktop-shortcut" => &mut self.desktop_shortcut,
            "menu-entry" => &mut self.menu_entry,
            "computer-speak" => &mut self.computer_speak,
            _ => return None,
        })
    }

    /// The flags that say where, to hand to an installer this one runs (`--user-wide`).
    pub fn mode_flag(&self) -> Option<&'static str> {
        self.mode.map(Mode::flag)
    }
}

/// What a part of an installer is.
#[derive(Clone, Copy)]
pub enum Kind {
    /// A program put in place under its name: the payload.
    Release(&'static Installation, &'static str),
    /// Something downloaded into the artifacts.
    Artifact(&'static Installation),
    /// A choice the installer's own code acts on.
    Choice,
    /// A path the installer's own code is given (`--waw DIR`).
    Path,
}

/// A part of an installer: its flag, its doc comment, what it is, whether it is on (`None`:
/// always) and, for a path, what it was given.
#[derive(Clone)]
pub struct Part {
    pub flag: &'static str,
    pub doc: &'static str,
    pub kind: Kind,
    pub on: Option<bool>,
    /// Whether it is on unless `--no-<flag>` (releases, artifacts, `#[On]` choices).
    pub on_by_default: bool,
    pub path: Option<PathBuf>,
}

impl Part {
    /// Whether the install does it.
    pub fn wanted(&self) -> bool {
        match self.kind {
            Kind::Path => self.path.is_some(),
            _ => self.on != Some(false),
        }
    }
}

/// An installer's command line, made by [`macro@crate::cli`].
pub trait Installer: Sized + DerefMut<Target = Flags> {
    /// The releases it puts in place, a field each.
    type Releases;
    /// The artifacts it downloads, a field each (and the artifacts' folder, through `Deref`).
    type Artifacts;
    /// What it installs, as the install says (`cli(title = ...)`, else the first release's name).
    const TITLE: &'static str;
    /// Its folder for `--user-wide` (`cli(folder = ...)`, else the first release's name).
    const FOLDER: &'static str;
    /// The struct's doc comment: what the installer installs.
    const ABOUT: &'static str;
    /// Its defaults, before the command line.
    fn blank() -> Self;
    /// Its parts, as they are now.
    fn parts(&self) -> Vec<Part>;
    /// Turns the part `flag` on or off; `false` if it has none by that name (or one always on).
    fn set(&mut self, flag: &str, on: bool) -> bool;
    /// Gives the path part `flag` its path; `false` if it has none by that name.
    fn set_path(&mut self, flag: &str, path: PathBuf) -> bool;
    /// The plan's lines for its parts (`cli(plan = function)`, else each wanted part's doc).
    fn plan(&self, places: &Places) -> Vec<String>;
    /// Its releases, as installed in `places`.
    fn releases(&self, places: &Places) -> Self::Releases;
    /// Its artifacts, as installed in `places`.
    fn artifacts(&self, places: &Places) -> Self::Artifacts;
}

/// The plan's lines for `cli`'s wanted parts, from their docs: what `cli(plan = ...)` replaces.
pub fn plan_parts<C: Installer>(cli: &C, places: &Places) -> Vec<String> {
    cli.parts()
        .iter()
        .filter(|part| part.wanted())
        .map(|part| match part.kind {
            Kind::Artifact(installation) => {
                Artifact::new(part.flag, installation, places, part.on).plan(part.doc)
            }
            Kind::Path => format!(
                "  - {}: {}",
                part.doc,
                part.path.as_deref().map(|path| path.display().to_string()).unwrap_or_default()
            ),
            _ => format!("  - {}", part.doc),
        })
        .collect()
}

/// The command line, read into the installer's struct: exits with the help for `--help`, or with
/// what is wrong.
pub fn parse<C: Installer>() -> C {
    let mut cli = C::blank();
    if let Err(problem) = apply(&mut cli, std::env::args().skip(1)) {
        eprintln!("{problem}\n\nSee --help for what it takes.");
        crate::screen::pause();
        std::process::exit(2);
    }
    cli
}

/// `args` read into `cli`, over what it says already (the answer to the question's flags win).
pub(crate) fn apply<C: Installer>(
    cli: &mut C,
    args: impl IntoIterator<Item = String>,
) -> Outcome<()> {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (flag, given) = match arg.split_once('=') {
            Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
            None => (arg, None),
        };
        let mut value = || {
            given
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        let name = flag.strip_prefix("--").unwrap_or_default();
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{}", help(cli));
                std::process::exit(0);
            }
            // (the later wins, so the answer's flag beats the command line's)
            "--locally" => cli.mode = Some(Mode::InPlace),
            "--user-wide" => cli.mode = Some(Mode::User),
            "--dir" => cli.dir = Some(PathBuf::from(value()?)),
            "--artifacts" => cli.artifacts = Some(PathBuf::from(value()?)),
            "--from" => cli.from = Some(PathBuf::from(value()?)),
            _ if cli.parts().iter().any(|part| part.flag == name && matches!(part.kind, Kind::Path)) => {
                cli.set_path(name, PathBuf::from(value()?));
            }
            _ => {
                if let Some(switch) = cli.switch(name) {
                    *switch = true;
                } else if let Some(name) = name.strip_prefix("no-")
                    && cli.set(name, false)
                {
                } else if !(cli.set(name, true)) {
                    return Err(format!("{flag} isn't one of this installer's flags"));
                }
            }
        }
    }
    Ok(())
}

/// Every flag the installer takes, for the question's answer (`y --user-wide`).
pub(crate) fn flag_names<C: Installer>(cli: &C) -> Vec<String> {
    let mut flags: Vec<String> = ["--locally", "--user-wide", "--dir", "--artifacts", "--from"]
        .into_iter()
        .map(String::from)
        .collect();
    flags.extend(
        SWITCHES
            .iter()
            .filter(|(flag, _)| *flag != "accept")
            .map(|(flag, _)| format!("--{flag}")),
    );
    for part in cli.parts() {
        match (part.kind, part.on) {
            (Kind::Path, _) => flags.push(format!("--{}", part.flag)),
            (_, Some(_)) => {
                flags.push(format!("--{}", part.flag));
                flags.push(format!("--no-{}", part.flag));
            }
            (_, None) => {}
        }
    }
    flags
}

/// The flags that change the plan, as its last line says them: each part's, then every install's.
pub(crate) fn changing<C: Installer>(cli: &C) -> String {
    let mut flags = vec!["--locally or --user-wide".to_string(), "--dir".into(), "--artifacts".into()];
    for part in cli.parts() {
        match (part.kind, part.on, part.on_by_default) {
            (Kind::Path, ..) => flags.push(format!("--{} {}", part.flag, part.flag.to_uppercase())),
            (_, None, _) => {}
            (_, Some(_), true) => flags.push(format!("--no-{}", part.flag)),
            (_, Some(_), false) => flags.push(format!("--{}", part.flag)),
        }
    }
    flags.extend(
        SWITCHES
            .iter()
            .filter(|(flag, _)| *flag != "accept")
            .map(|(flag, _)| format!("--{flag}")),
    );
    flags.join(", ")
}

/// The `--help`.
fn help<C: Installer>(cli: &C) -> String {
    let mut lines = vec![C::ABOUT.to_string(), String::new(), "Its parts:".to_string()];
    let line = |flag: String, doc: &str| format!("  {flag:<26} {doc}");
    for part in cli.parts() {
        lines.push(match (part.kind, part.on, part.on_by_default) {
            (Kind::Path, ..) => line(format!("--{} {}", part.flag, part.flag.to_uppercase()), part.doc),
            (_, None, _) => line("(always)".into(), part.doc),
            (_, Some(_), true) => line(format!("--no-{}", part.flag), &format!("{} (unless given)", part.doc)),
            (_, Some(_), false) => line(format!("--{}", part.flag), part.doc),
        });
    }
    lines.push(String::new());
    lines.push("Every install's:".into());
    lines.push(line(
        "--locally".into(),
        "Installs in place, where this program is (or --from). This or --user-wide is needed",
    ));
    lines.push(line(
        "--user-wide".into(),
        "Installs for you only, in your own folder, its command on your PATH. This or --locally is needed",
    ));
    lines.push(line("--dir DIR".into(), "The folder it goes in, rather than the mode's"));
    lines.push(line(
        "--artifacts DIR".into(),
        "Where everything else goes (downloads, the log), rather than artifacts/ beside it",
    ));
    lines.push(line(
        "--from DIR".into(),
        "Installs the release in DIR, as if this ran from there",
    ));
    for (flag, doc) in SWITCHES {
        lines.push(line(format!("--{flag}"), doc));
    }
    lines.join("\n")
}
