//! installation-groans: a ratatui library for the workspace's installers, speaking in its voices.
//!
//! An installer is a struct of what it installs, made a command line by [`macro@cli`]; [`run`]
//! opens the screen, [`prompt_user`] asks the one question (its plan under a red warning, its parts
//! to tick, "Are you sure? [y/n]" in the voice's words) and gives the [`Location`] it installs in,
//! whose [`Location::install`] puts its [`Release`]s in place and downloads its artifacts, saying
//! all of it through the [`journal`] (the voices built in):
//!
//! ```ignore
//! use installation_groans::Installation;
//!
//! const TDL: Installation = Installation {
//!     git: Some("https://github.com/error-error-fruit-user/tdl.git"),
//!     ..Installation::default()
//! };
//!
//! /// Terminal launcher for UZDoom
//! #[installation_groans::cli]
//! struct Cli {
//!     /// Installs tdl
//!     #[installation_groans::Release(TDL, "tdl")]
//!     tdl: (),
//! }
//!
//! #[installation_groans::location(Cli)]
//! struct Location;
//!
//! fn main() {
//!     installation_groans::run(|terminal| {
//!         let mut cli = Cli::new();
//!         if let Some(location) = installation_groans::prompt_user(terminal, &mut cli) {
//!             location.install(terminal, &cli);
//!         }
//!     });
//! }
//! ```
//!
//! Run by another script (`GROANS_*`), or with no terminal, there is no screen: the question is
//! the journal's own (asked of no one when nested), and everything is said line by line.

pub use installation_groans_macros::{cli, location};
pub use ratatui;
pub use toml;

/// The journal every installer speaks through, with `say!`, `warn!` and `note!`.
pub mod journal;

mod command_line;
mod installation;
mod places;
mod prompt;
mod release;
mod screen;

pub use command_line::{Flags, Installer, Kind, Mode, Part, lowered, parse, plan_parts};
pub use installation::{Artifact, Installation};
pub use places::{Location, Places};
pub use prompt::prompt_user;
pub use release::Release;
pub use screen::{Screen, fail, run, then};

/// Toiling Through Hell, the voice the install speaks in unless told `--computer-speak`.
pub const TOILING_THROUGH_HELL: &str = include_str!("../toiling-through-hell.toml");
/// Computer Speak, for `--computer-speak`.
pub const COMPUTER_SPEAK: &str = include_str!("../computer-speak.toml");

/// What went wrong, for the user.
type Outcome<T> = Result<T, String>;
