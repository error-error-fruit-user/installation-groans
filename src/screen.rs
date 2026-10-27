//! The installer's run: [`run`], and the [`Screen`] it draws on.

use std::{io::IsTerminal, process::Command, sync::Mutex};

use ratatui::DefaultTerminal;

use crate::journal;

/// The terminal, drawn on with ratatui while the question is asked (opened then, so what comes
/// before, `--help` or a flag that's wrong, is said plainly); none when the installer is run by
/// another script or not in a terminal, and none once the work begins and the journal speaks.
pub struct Screen {
    shown: bool,
    terminal: Option<DefaultTerminal>,
}

impl Screen {
    /// The ratatui terminal, opened the first time it is asked for, while there can be one.
    pub fn terminal(&mut self) -> Option<&mut DefaultTerminal> {
        if self.shown && self.terminal.is_none() {
            self.terminal = Some(ratatui::init());
        }
        self.terminal.as_mut()
    }

    /// Gives the terminal back to the journal's lines, for good.
    pub fn leave(&mut self) {
        self.shown = false;
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
    /// What runs once the install is over ([`then`]).
    pub then: Option<Box<dyn FnOnce() + Send>>,
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

/// Says `problem` and has the run end in failure (the install goes on unless the caller stops).
pub fn fail(problem: &str) {
    crate::warn!("{problem}");
    if let Ok(mut state) = STATE.lock() {
        state.failed = true;
    }
}

/// Runs `then` once the install is over: after the screen is gone and the end is said, unless
/// something failed (opening the program installed, as the player would).
pub fn then(then: impl FnOnce() + Send + 'static) {
    if let Ok(mut state) = STATE.lock() {
        state.then = Some(Box::new(then));
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
        pause();
        std::process::exit(1);
    }
    let shown = !nested() && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let mut screen = Screen {
        shown,
        terminal: None,
    };
    main(&mut screen);
    screen.leave();
    let state = std::mem::take(&mut *STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    if let Some(what) = state.began {
        journal::finish(what);
    }
    pause();
    if let Some(then) = state.then
        && !state.failed
    {
        then();
    }
    std::process::exit(if state.failed || state.declined { 1 } else { 0 })
}

/// The end of every run, before anything else happens (opening what was installed, or the window
/// closing): a line to press Enter on, so what was done, or what went wrong, can be read. Not when
/// another script runs this one (its own end waits) or there is no one at the terminal.
pub(crate) fn pause() {
    if nested() || !std::io::stdin().is_terminal() {
        return;
    }
    let said = journal::phrase("pause", &[]).unwrap_or_else(|| "Press Enter to continue.".into());
    print!("{} ", journal::paint(&said, "pause"));
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::stdin().read_line(&mut String::new());
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
