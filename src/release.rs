//! The programs an install puts in place, its payload: [`Release`].

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    Installation, Mode, Outcome, Places,
    installation::{self, Scratch},
    journal, note,
};

/// A program the install puts in place as `name`, from the nearest place it is found: beside the
/// installer (a release shipped with it), the workspace's checkout of it (`<name>/` beside a folder
/// the installer is in, built with cargo, else its `published/releases/`), its download, else its
/// git repository (cloned into `sources/` in the artifacts and built).
pub struct Release {
    /// Its name: the program's, and its command's.
    pub name: &'static str,
    /// Where it is once installed.
    pub path: PathBuf,
    /// Whether the install puts it in place.
    pub on: bool,
    installation: &'static Installation,
    places: Places,
}

/// The names `name` may have as a file: the program, with `.exe` on Windows, or an AppImage.
fn names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![format!("{name}.exe")]
    } else {
        vec![name.to_string(), format!("{name}.AppImage")]
    }
}

/// The program `name` in `dir`, if it is there.
fn found_in(dir: &Path, name: &str) -> Option<PathBuf> {
    names(name)
        .into_iter()
        .map(|file| dir.join(file))
        .find(|path| path.is_file())
}

impl Release {
    /// `name`, from `installation`, as installed in `places` (`on`: `None` for always).
    pub fn new(
        name: &'static str,
        installation: &'static Installation,
        places: &Places,
        on: Option<bool>,
    ) -> Self {
        let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        // 🧱 in place, one beside already (an AppImage among them) stays where and what it is
        let path = match places.mode {
            Mode::InPlace => found_in(&places.dir, name),
            Mode::User => None,
        };
        Release {
            name,
            path: path.unwrap_or_else(|| places.dir.join(file)),
            on: on != Some(false),
            installation,
            places: places.clone(),
        }
    }

    /// Where it comes from, building it if that is how it is had.
    fn source(&self, scratch: &Scratch) -> Outcome<PathBuf> {
        if let Some(beside) = found_in(&self.places.here, self.name) {
            note!("{}: beside this program, {}", self.name, beside.display());
            return Ok(beside);
        }
        // 🏘️ the workspace's checkout, so unpushed work is never replaced by an older origin
        let mut looked = Vec::new();
        for folder in self.places.here.ancestors().skip(1) {
            let checkout = folder.join(self.name);
            if looked.contains(&checkout) {
                continue;
            }
            looked.push(checkout.clone());
            if checkout.join("Cargo.toml").is_file()
                && let Ok(built) = self.build(&checkout)
            {
                return Ok(built);
            }
            if let Some(released) = found_in(&checkout.join("published").join("releases"), self.name) {
                note!("{}: the workspace's release, {}", self.name, released.display());
                return Ok(released);
            }
        }
        if let Some(download) = self.installation.download {
            let name = download.rsplit('/').next().unwrap_or(self.name);
            return installation::fetch(self.installation, name, scratch);
        }
        if let Some(git) = self.installation.git {
            let sources = self.places.artifacts.join("sources").join(self.name);
            let (status, shown) = if sources.join(".git").is_dir() {
                (
                    journal::run(
                        Command::new("git").arg("-C").arg(&sources).args(["pull", "--ff-only"]),
                        &format!("git pull ({})", self.name),
                    ),
                    "git pull",
                )
            } else {
                let _ = fs::remove_dir_all(&sources);
                (
                    journal::run(
                        Command::new("git").args(["clone", "--depth", "1", git]).arg(&sources),
                        &format!("git clone {git}"),
                    ),
                    "git clone",
                )
            };
            match status {
                Ok(status) if status.success() => return self.build(&sources),
                Ok(status) => return Err(format!("{shown} failed ({status})")),
                Err(error) => return Err(format!("couldn't run git: {error}")),
            }
        }
        Err(format!(
            "it is nowhere to be found: not beside this program ({}), in a checkout of it, or anywhere to download it from",
            self.places.here.display()
        ))
    }

    /// Builds it in `checkout` with cargo. Returns the program.
    fn build(&self, checkout: &Path) -> Outcome<PathBuf> {
        journal::section("build");
        let status = journal::run(
            Command::new("cargo")
                .args(["build", "--release", "--bin", self.name, "--manifest-path"])
                .arg(checkout.join("Cargo.toml")),
            &format!("cargo build --release --bin {} ({})", self.name, checkout.display()),
        )
        .map_err(|error| format!("couldn't run cargo: {error}"))?;
        if !status.success() {
            return Err(format!("cargo build failed ({status})"));
        }
        found_in(&checkout.join("target").join("release"), self.name)
            .ok_or_else(|| format!("cargo built no {} in {}", self.name, checkout.display()))
    }

    /// Puts it in place (and its command on your PATH for `--mode user`).
    pub(crate) fn install(&self, scratch: &Scratch) -> Outcome<()> {
        let from = self.source(scratch)?;
        journal::section("install");
        if from != self.path {
            installation::place(&from, &self.path, true)?;
            for license in ["LICENSE", "THIRD_PARTY_LICENSES.txt"] {
                if let Some(folder) = from.parent()
                    && folder.join(license).is_file()
                    && folder != self.places.dir
                {
                    let _ = fs::copy(folder.join(license), self.places.dir.join(license));
                    note!("copied {license} into {}", self.places.dir.display());
                }
            }
        }
        journal::tell(
            "copy",
            &[("what", self.name), ("place", &self.path.display().to_string())],
            &format!("{}: {}", self.name, self.path.display()),
        );
        if self.places.mode == Mode::User {
            let said = put_on_path(self.name, &self.path, &self.places)?;
            journal::tell(
                "path",
                &[("place", &self.places.bin().display().to_string())],
                &said,
            );
        }
        Ok(())
    }

    /// Runs it with `args` (`&[&"setup", &location.artifact("setup.toml")]`), its lines in the
    /// voice, once: a run that did it already skips it (`--reinstall` does it again). Returns
    /// whether it went well.
    pub fn run_with(&self, args: &[&dyn AsRef<OsStr>]) -> bool {
        let shown: Vec<String> = std::iter::once(self.name.to_string())
            .chain(args.iter().map(|arg| arg.as_ref().to_string_lossy().into_owned()))
            .collect();
        let shown = shown.join(" ");
        let step = format!("ran: {shown}");
        let mut steps = journal::Steps::open(&self.places.artifacts, self.places.reinstall);
        if steps.done(&step) {
            steps.skip(&shown, &self.places.artifacts.display().to_string());
            return true;
        }
        let mut command = Command::new(&self.path);
        command.args(args.iter().map(|arg| arg.as_ref()));
        match journal::run(&mut command, &shown) {
            Ok(status) if status.success() => {
                steps.finish(&step);
                true
            }
            Ok(status) => {
                crate::warn!("{shown} failed ({status})");
                false
            }
            Err(error) => {
                crate::warn!("Couldn't run {}: {error}", self.path.display());
                false
            }
        }
    }

    /// Runs it by its command once the install is over, the terminal handed over to it, as the
    /// player would.
    pub fn run_alias(&self) {
        let (name, path) = (self.name, self.path.clone());
        crate::then(move || {
            journal::section("play");
            journal::tell(
                "run",
                &[("command", name)],
                &format!("Opening {name}: {}", path.display()),
            );
            if let Err(error) = Command::new(&path).status() {
                crate::warn!("Couldn't run {}: {error}", path.display());
            }
        });
    }
}

/// Makes `name` (at `program`) a command anywhere in the terminal. Returns what to tell the user.
fn put_on_path(name: &str, program: &Path, places: &Places) -> Outcome<String> {
    #[cfg(windows)]
    {
        let _ = (name, program);
        windows_path(&places.dir)
    }
    #[cfg(not(windows))]
    {
        unix_path(name, program, places)
    }
}

#[cfg(not(windows))]
fn unix_path(name: &str, program: &Path, places: &Places) -> Outcome<String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("$HOME isn't set")?;
    let bin = places.bin();
    let link = bin.join(name);
    let problem = |error: io::Error| format!("{}: {error}", link.display());
    fs::create_dir_all(&bin).map_err(problem)?;
    match fs::symlink_metadata(&link) {
        Ok(meta) if meta.file_type().is_symlink() => fs::remove_file(&link).map_err(problem)?,
        Ok(_) => {
            return Err(format!(
                "{} is a file that isn't a link, so it is left alone",
                link.display()
            ));
        }
        Err(_) => {}
    }
    std::os::unix::fs::symlink(program, &link).map_err(problem)?;
    note!("linked {} to {}", link.display(), program.display());
    let mut said = format!("The {name} command: {}", link.display());
    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|entry| entry == bin));
    if !on_path {
        let profile = home.join(".profile");
        let mark = format!("# added by install-{name}");
        let old = fs::read_to_string(&profile).unwrap_or_default();
        if old.contains(&mark) {
            said.push_str(&format!(
                "\n{} isn't on this terminal's PATH yet; {} adds it, so open a new terminal.",
                bin.display(),
                profile.display()
            ));
        } else {
            let separator = if old.is_empty() || old.ends_with('\n') { "" } else { "\n" };
            let line = format!("{separator}{mark}\nexport PATH=\"{}:$PATH\"\n", bin.display());
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&profile)
                .and_then(|mut file| io::Write::write_all(&mut file, line.as_bytes()))
                .map_err(|error| format!("{}: {error}", profile.display()))?;
            note!("added a line putting {} on the PATH to {}", bin.display(), profile.display());
            said.push_str(&format!(
                "\n{} wasn't on your PATH, so a line adding it is now in {}. Open a new terminal (or run `. ~/.profile`) to use {name} anywhere.",
                bin.display(),
                profile.display()
            ));
        }
    }
    Ok(said)
}

/// Adds `dir` to the user's PATH, through PowerShell, which is how the environment is changed so
/// that every program sees it.
#[cfg(windows)]
fn windows_path(dir: &Path) -> Outcome<String> {
    let script = "$d = $env:GROANS_DIR; $p = [Environment]::GetEnvironmentVariable('Path', 'User'); \
         if (($p -split ';') -notcontains $d) { \
           [Environment]::SetEnvironmentVariable('Path', ($p.TrimEnd(';') + ';' + $d), 'User') }";
    note!("adding {} to the user PATH through powershell", dir.display());
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("GROANS_DIR", dir)
        .status()
        .map_err(|error| format!("couldn't run powershell: {error}"))?;
    if !status.success() {
        return Err(format!("powershell failed ({status})"));
    }
    Ok(format!(
        "{} is on your PATH now. Open a new terminal to use it anywhere.",
        dir.display()
    ))
}
