//! The command line every installer has: [`Flags`], and its own parts from [`macro@crate::cli`].

use std::{ops::DerefMut, path::PathBuf};

use crate::{Installation, Outcome, Places};

/// Where an install puts its payload.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// Where the installer is (or `--dir`): nothing is copied elsewhere and no command is added.
    #[default]
    InPlace,
    /// Copied into your own folder (`~/.local/opt/<name>`, `%LOCALAPPDATA%\Programs\<name>`), with
    /// its command on your PATH.
    User,
}

/// The flags every install has, read through the installer's struct (`cli.run`).
#[derive(Clone, Debug, Default)]
pub struct Flags {
    /// `--accept`: goes ahead without asking, with the defaults for what isn't given.
    pub accept: bool,
    /// `--mode in-place|user`.
    pub mode: Mode,
    /// `--dir DIR`: the folder the payload goes in.
    pub dir: Option<PathBuf>,
    /// `--artifacts DIR`: where everything else goes.
    pub artifacts: Option<PathBuf>,
    /// `--reinstall`: does everything again, where a run picks up where the last left off.
    pub reinstall: bool,
    /// `--run`: runs the program installed once it is done.
    pub run: bool,
    /// `--desktop-shortcut`.
    pub desktop_shortcut: bool,
    /// `--menu-entry`.
    pub menu_entry: bool,
    /// `--computer-speak`: speaks plainly, rather than as a descent through hell.
    pub computer_speak: bool,
}

/// The flags of [`Flags`] that are on or off, with their help, in the order they are shown.
const SWITCHES: [(&str, &str); 6] = [
    ("accept", "Goes ahead without asking, with the defaults for what isn't given"),
    ("reinstall", "Does everything again, where a run picks up where the last one left off"),
    ("run", "Runs it once it is installed"),
    ("desktop-shortcut", "Makes a shortcut to it on the desktop, with the system's lock icon"),
    ("menu-entry", "Makes an entry for it in the app menu (the Start menu on Windows)"),
    ("computer-speak", "Says what it does plainly and formally, rather than as a descent through hell"),
];

impl Flags {
    /// The switch named `flag` (as on the command line, `menu-entry`), if there is one.
    pub fn switch(&mut self, flag: &str) -> Option<&mut bool> {
        Some(match flag {
            "accept" => &mut self.accept,
            "reinstall" => &mut self.reinstall,
            "run" => &mut self.run,
            "desktop-shortcut" => &mut self.desktop_shortcut,
            "menu-entry" => &mut self.menu_entry,
            "computer-speak" => &mut self.computer_speak,
            _ => return None,
        })
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
}

/// A part of an installer: its flag, its doc comment, what it is and whether it is on (`None`:
/// always).
#[derive(Clone, Copy)]
pub struct Part {
    pub flag: &'static str,
    pub doc: &'static str,
    pub kind: Kind,
    pub on: Option<bool>,
}

impl Part {
    /// Whether the install does it.
    pub fn wanted(&self) -> bool {
        self.on != Some(false)
    }
}

/// An installer's command line, made by [`macro@crate::cli`].
pub trait Installer: Sized + DerefMut<Target = Flags> {
    /// The releases it puts in place, a field each.
    type Releases;
    /// The first release's name: the program it installs (and its folder for `--mode user`).
    const NAME: &'static str;
    /// The struct's doc comment: what the installer installs.
    const ABOUT: &'static str;
    /// Its defaults, before the command line.
    fn blank() -> Self;
    /// Its parts, as they are now.
    fn parts(&self) -> Vec<Part>;
    /// Turns the part `flag` on or off; `false` if it has none by that name (or one always on).
    fn set(&mut self, flag: &str, on: bool) -> bool;
    /// Its releases, as installed in `places`.
    fn releases(&self, places: &Places) -> Self::Releases;
}

/// The command line, read into the installer's struct: exits with the help for `--help`, or with
/// what is wrong.
pub fn parse<C: Installer>() -> C {
    let mut cli = C::blank();
    if let Err(problem) = apply(&mut cli, std::env::args().skip(1)) {
        eprintln!("{problem}\n\nSee --help for what it takes.");
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
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{}", help(cli));
                std::process::exit(0);
            }
            "--mode" => {
                cli.mode = match value()?.as_str() {
                    "in-place" => Mode::InPlace,
                    "user" => Mode::User,
                    other => return Err(format!("--mode is in-place or user, not {other}")),
                }
            }
            "--dir" => cli.dir = Some(PathBuf::from(value()?)),
            "--artifacts" => cli.artifacts = Some(PathBuf::from(value()?)),
            _ => {
                let name = flag.strip_prefix("--").unwrap_or_default();
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

/// Every flag the installer takes, for the question's answer (`y --mode user`).
pub(crate) fn flag_names<C: Installer>(cli: &C) -> Vec<String> {
    let mut flags: Vec<String> = ["--mode", "--dir", "--artifacts"]
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
        if part.on.is_some() {
            flags.push(format!("--{}", part.flag));
            flags.push(format!("--no-{}", part.flag));
        }
    }
    flags
}

/// The `--help`.
fn help<C: Installer>(cli: &C) -> String {
    let mut lines = vec![C::ABOUT.to_string(), String::new(), "Its parts:".to_string()];
    let line = |flag: String, doc: &str| format!("  {flag:<26} {doc}");
    for part in cli.parts() {
        lines.push(match (part.kind, part.on) {
            (_, None) => line("(always)".into(), part.doc),
            (Kind::Choice, Some(_)) => line(format!("--{}", part.flag), part.doc),
            (_, Some(_)) => line(
                format!("--no-{}", part.flag),
                &format!("{} (unless given)", part.doc),
            ),
        });
    }
    lines.push(String::new());
    lines.push("Every install's:".into());
    lines.push(line(
        "--mode in-place|user".into(),
        "Where it goes: where this program is, or your own folder with its command on your PATH",
    ));
    lines.push(line("--dir DIR".into(), "The folder it goes in, rather than the mode's"));
    lines.push(line(
        "--artifacts DIR".into(),
        "Where everything else goes (downloads, the log), rather than artifacts/ beside it",
    ));
    for (flag, doc) in SWITCHES {
        lines.push(line(format!("--{flag}"), doc));
    }
    lines.join("\n")
}
