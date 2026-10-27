# Installation Groans

> **AI notice:** this project is developed with the help of AI. Much of the code and documentation was written with [Claude](https://claude.ai) (Anthropic), working under my direction and review. Commits that Claude helped write carry a `Co-Authored-By: Claude` trailer, so you can see which ones in the history.

The voices the installers of these projects speak in while they work: what they say when they begin, download, check, unpack, copy, run something, fail and finish. Each is a TOML of phrases per event, one picked at random each time:

- [`toiling-through-hell.toml`](toiling-through-hell.toml): **Toiling Through Hell**, the install as your computer's descent through hell, dragging every file up out of the pit. The default.
- [`computer-speak.toml`](computer-speak.toml): **Computer Speak**, a plain and formal line per step, for those who pass `--computer-speak`.

Every voice also has:

- `[colors]`: each event's colour (terminal SGR codes), and `other` (what other programs print), `section` and `warning`.
- `[sections]`: the names of the parts of the work, shown as yellow `--- Section ---` separators (`build`, `pack`, `install`, `download`, `setup`, `song`, `weapons`, `shortcuts`, `play`).
- `[[rewrite]]`: rules for what other programs print (cargo, git, Blender, tdl...): a regex `pattern`, `phrases` with `{1}`, `{2}`... for its groups and `{line}` for the line, and a `color`. A line no rule catches gets the `[fallback]` `format`, so nothing comes through untouched.
- `[strain]` (Toiling Through Hell only): when another program's lines pour out faster than its `threshold` a second, a line under them pulses with how hard the machine labours, its `frames` in turn with a `{bar}` filled by the rate, throbbing through its `colors`.
- The question every install asks once, `confirm` (it always ends in `[y/n]`), with `unclear`, `unknown_flags` and `declined` for what follows a bad answer or a no, and `pause`, the line every run ends on, to press Enter on once what was done has been read.

Two modules speak them, so every installer and script does it the same way, and ends the same way, with a press of Enter:

- [`groans.py`](groans.py), for the Python scripts (`publish.py`, `publish-install.py`, `appimage.py`): `start`, `say`, `section`, `warning`, `confirm`, `run` (another program, rewritten in the voice), `capture`, `built`, `shortcut`, `finish` and `pause`.
- [`src/journal.rs`](src/journal.rs), the journal of the library below, which every Rust installer is built with: the same, as `tell`, `section`, `warning`, `confirm`, `run`, `shortcut`, `finish`, and `say!`/`warn!`/`note!`.

## The library

This repository is also a Rust crate, `installation-groans`: a [ratatui](https://ratatui.rs) library every installer of these projects is written with (`install-tdl`, `install-nacht`), whose code is mostly macros and consts. An installer is a struct of what it installs:

```rust
use installation_groans::Installation;

const TDL: Installation = Installation {
    git: Some("https://github.com/error-error-fruit-user/tdl.git"),
    ..Installation::default()
};

/// Terminal launcher for UZDoom
#[installation_groans::cli]
struct Cli {
    /// Installs tdl
    #[installation_groans::Release(TDL, "tdl")]
    tdl: (),
    /// Installs UZDoom
    #[installation_groans::Artifact(UZDOOM)]
    uzdoom: bool,
    /// Keeps UZDoom as tdl's (tdl setup)
    #[installation_groans::On]
    setup: bool,
}

#[installation_groans::location(Cli)]
struct Location;

fn main() {
    installation_groans::run(|terminal| {
        let mut cli = Cli::new();
        if let Some(location) = installation_groans::prompt_user(terminal, &mut cli) {
            location.install(terminal, &cli);
            if cli.setup && location.artifacts.uzdoom.exists() {
                // location.artifacts.uzdoom.relative() is "uzdoom/uzdoom.AppImage", for setup.toml
                location
                    .releases
                    .tdl
                    .run_with(&[&"setup", &location.artifact("setup.toml")]);
            }
        }
    });
}
```

- An `Installation` is a `const`: where it comes from (`git`, `download`), what the download must be (`size`, `blake3`), and where it goes (`into` a folder of the artifacts, the files to `take` out of a zip, the `file` that matters, whether it is `executable`). One that differs between systems is two, picked with `cfg!`.
- `#[installation_groans::cli]` makes the command line out of the struct's fields and their doc comments (the help, the plan and the question all use them). A `Release` field is a program put in place, the payload: from beside the installer (or `--from`), else the workspace's checkout of it (built with cargo, else its `published/releases/`), else its download, else its git repository. An `Artifact` field is downloaded into the artifacts, checked and unpacked. Both are on unless `--no-<field>`. A plain `bool` is a choice for the installer's own code, off unless `--<field>`, or on unless `--no-<field>` with `#[installation_groans::On]`; an `Option<PathBuf>` is a path it is given (`--waw PATH`); `#[installation_groans::Flag("name")]` gives a field another flag. `cli(title = "...", folder = "...", plan = function)` says what the install calls what it installs, its folder for `--user-wide`, and the plan's lines for its parts, when its first release's name and the docs won't do.
- Every installer has the same flags besides its own: `--locally` or `--user-wide` (where: never a default, and without either nothing is done), `--dir`, `--artifacts`, `--from`, `--accept`, `--reinstall`, `--desktop-shortcut`, `--menu-entry` and `--computer-speak`, read through the struct (`cli.reinstall`).
- `installation_groans::run` refuses root and ends the run: done and where the log is, then a line to press Enter on, so what was done or what went wrong can be read before anything else happens (what `then` or `run_alias` opens, or the window closing), then the exit code.
- `prompt_user` asks the one question on the screen: the plan under a red warning, where and the parts and switches to tick, "Are you sure? [y/n]" in the voice's words. Run by another script or without a terminal, it is the journal's question instead (whose y can carry flags); `--accept` skips it.
- `location.install` gives the screen up to the journal, puts the releases in place (and on your PATH for `--user-wide`), downloads the artifacts and makes the shortcuts. `location.artifacts.<field>` is an artifact as installed (`.path`, `.relative()`, `.exists()`), `location.artifact(name)` a file in the artifacts, `location.releases.<field>.run_with(&[...])` runs a release once (the next run skips it, unless `--reinstall`) and `.run_alias()` opens it at the end; `installation_groans::fail` says a problem and has the run fail.
- The journal is the crate's `journal` module, with `say!`, `warn!` and `note!`, and both voices are built in, so an installer needs nothing but the crate.

A script run by another (publish.py by publish-install.py, install-tdl by install-nacht) learns it from the environment (`GROANS_JOURNAL`, `GROANS_DEPTH`, `GROANS_VOICE`, `GROANS_COLOR`, `GROANS_SECTION`, `GROANS_CAPTURED`): it speaks in the same voice and colours, writes into the same log, says nothing the outer one already says (its beginning and end, where the log is, its warning, a section already open), and asks nothing.

The projects that install things carry this repository as a git submodule, `installation-groans/`: their installers depend on the crate there (`installation-groans = { path = "../installation-groans" }`), and their Python scripts load `groans.py` from it (else from a checkout beside them, else GitHub).

## License

[0BSD](LICENSE).
