//! The installer's run: [`run`], and the [`Screen`] it draws on.

use std::{io::IsTerminal, path::PathBuf, process::Command, sync::Mutex};

use ratatui::DefaultTerminal;

use crate::journal;

/// The terminal, drawn on with ratatui while the question is asked; none when the installer is
/// run by another script or not in a terminal, and none once the work begins and the journal
/// speaks.
pub struct Screen {
    terminal: Option<DefaultTerminal>,
}

impl Screen {
    /// The ratatui terminal, while there is one.
    pub fn terminal(&mut self) -> Option<&mut DefaultTerminal> {
        self.terminal.as_mut()
    }

    /// Gives the terminal back to the journal's lines.
    pub fn leave(&mut self) {
        if self.terminal.take().is_some() {
            ratatui::restore();
        }
    }
}

/// How the run went, for its end.
#[derive(Default)]
pub(crate) struct State {
    /// What the install began installing, once it did.
    pub began: Option<&'static str>,
    pub failed: bool,
    pub declined: bool,
    /// What [`crate::Release::run_alias`] runs at the end.
    pub then: Option<PathBuf>,
}

pub(crate) static STATE: Mutex<State> = Mutex::new(State {
    began: None,
    failed: false,
    declined: false,
    then: None,
});

/// The install began, installing `what`: its end says it is done.
pub(crate) fn began(what: &'static str) {
    if let Ok(mut state) = STATE.lock() {
        state.began = Some(what);
    }
}

/// Whether another installation-groaned script runs this one.
pub(crate) fn nested() -> bool {
    std::env::var("GROANS_DEPTH")
        .ok()
        .and_then(|depth| depth.parse::<u32>().ok())
        .is_some_and(|depth| depth > 0)
}

/// Runs the installer, `main`, on the screen, then ends it: done and where the log is (when it
/// began), what [`crate::Release::run_alias`] asked for, and the exit, a failure if anything
/// failed or the answer was no. Refuses to run as root: installs are in place or for the user.
pub fn run(main: impl FnOnce(&mut Screen)) -> ! {
    if root() {
        eprintln!("Don't run this as root (with sudo): it installs in place or for you, and needs no root.");
        std::process::exit(1);
    }
    let shown = !nested() && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let mut screen = Screen {
        terminal: shown.then(ratatui::init),
    };
    main(&mut screen);
    screen.leave();
    let state = std::mem::take(&mut *STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    if let Some(what) = state.began {
        journal::finish(what);
    }
    if let Some(program) = state.then.filter(|_| !state.failed) {
        crate::note!("running {}", program.display());
        if let Err(error) = Command::new(&program).status() {
            crate::warn!("Couldn't run {}: {error}", program.display());
        }
    }
    std::process::exit(if state.failed || state.declined { 1 } else { 0 })
}

/// Whether this runs as root, whose install nobody else could use or change.
fn root() -> bool {
    #[cfg(not(windows))]
    {
        Command::new("id")
            .arg("-u")
            .output()
            .is_ok_and(|output| output.stdout.trim_ascii() == b"0")
    }
    #[cfg(windows)]
    {
        false
    }
}
