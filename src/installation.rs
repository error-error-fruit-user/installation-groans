//! What an installer installs, [`Installation`], and getting one: downloads checked against their
//! size and hash, unpacked or taken out of their zips.

use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use crate::{Outcome, journal, note};

/// Something an installer installs, as a `const`, filled in from [`Installation::default`]:
///
/// ```ignore
/// const FREEDOOM: Installation = Installation {
///     name: "Freedoom 2",
///     version: "0.13.0",
///     download: Some("https://github.com/freedoom/freedoom/releases/download/v0.13.0/freedoom-0.13.0.zip"),
///     size: 24_143_781,
///     blake3: Some("5f947833ecce402da78af8f63e9d647191b48f97dfa0ebd16b464548492d9719"),
///     into: "wads/base_wads",
///     take: &[("freedoom-0.13.0/freedoom2.wad", "freedoom2.wad")],
///     ..Installation::default()
/// };
/// ```
///
/// One that differs between systems is two, picked with `cfg!`: `const UZDOOM: Installation = if
/// cfg!(windows) { UZDOOM_WINDOWS } else { UZDOOM_LINUX };`.
#[derive(Clone, Copy, Debug)]
pub struct Installation {
    /// What it is called where the install says so; its field's name if empty.
    pub name: &'static str,
    /// Its version, for the plan.
    pub version: &'static str,
    /// Its repository: a release is built from it (`cargo build --release`) when it is found
    /// nowhere nearer; an artifact is cloned into its folder.
    pub git: Option<&'static str>,
    /// Where it is downloaded from: a zip is unpacked (or has `take` taken out), a tarball
    /// (`.tar.xz`, `.tar.gz`, `.tgz`, `.tar`) unpacked with `tar`, anything else kept as it is.
    pub download: Option<&'static str>,
    /// Other places with the same file, tried in turn when `download` fails.
    pub mirrors: &'static [&'static str],
    /// The download's size in bytes, which it must be (0: not checked).
    pub size: u64,
    /// The download's BLAKE3 hash, which it must have.
    pub blake3: Option<&'static str>,
    /// The download's SHA-256 hash, which it must have (for a release that publishes one).
    pub sha256: Option<&'static str>,
    /// An artifact's folder in the artifacts (its field's name if empty).
    pub into: &'static str,
    /// The files taken out of a zip, and what each is called in `into`; empty unpacks it all.
    pub take: &'static [(&'static str, &'static str)],
    /// The file that matters in `into` (what a download that isn't a zip is kept as, or the program
    /// in an unpacked zip); the first file taken, or the download's own name, if empty.
    pub file: &'static str,
    /// Whether `file` is a program, to be let run.
    pub executable: bool,
}

impl Installation {
    /// Nothing at all, to fill in with `..Installation::default()`.
    pub const fn default() -> Self {
        Installation {
            name: "",
            version: "",
            git: None,
            download: None,
            mirrors: &[],
            size: 0,
            blake3: None,
            sha256: None,
            into: "",
            take: &[],
            file: "",
            executable: false,
        }
    }

    /// What it is called, its field's (`flag`) name if it says nothing.
    pub fn called<'a>(&'a self, flag: &'a str) -> &'a str {
        if self.name.is_empty() { flag } else { self.name }
    }

    /// The download's own file name.
    fn download_name(&self) -> Option<&'static str> {
        self.download?.rsplit('/').next().filter(|name| !name.is_empty())
    }

    /// Its folder, as an artifact of the install whose artifacts are `art`.
    pub fn folder(&self, art: &Path, flag: &str) -> PathBuf {
        art.join(if self.into.is_empty() { flag } else { self.into })
    }

    /// The file that matters in its folder, as an artifact in `art`.
    pub fn path(&self, art: &Path, flag: &str) -> PathBuf {
        let folder = self.folder(art, flag);
        if !self.file.is_empty() {
            folder.join(self.file)
        } else if let Some((_, name)) = self.take.first() {
            folder.join(name)
        } else if let Some(name) = self.download_name().filter(|name| !unpacked(name)) {
            folder.join(name)
        } else {
            folder
        }
    }

    /// Puts it in its folder in `art` (`flag` is its field), unless it is there already (and not
    /// `reinstall`). Returns the file that matters, or `None` if nothing was done.
    pub(crate) fn install(
        &self,
        flag: &str,
        art: &Path,
        scratch: &Scratch,
        reinstall: bool,
    ) -> Outcome<Option<PathBuf>> {
        let name = self.called(flag);
        let folder = self.folder(art, flag);
        let path = self.path(art, flag);
        let there = if self.take.is_empty() {
            folder.exists()
        } else {
            self.take.iter().all(|(_, to)| folder.join(to).exists())
        };
        if there && !reinstall {
            journal::tell(
                "skip",
                &[("what", name), ("place", &folder.display().to_string())],
                &format!("{name}: {} is there already, so it is left alone", folder.display()),
            );
            return Ok(None);
        }
        let Some(download_name) = self.download_name() else {
            if let Some(git) = self.git {
                clone(git, &folder)?;
                return Ok(Some(path));
            }
            return Err(format!("{name} has nowhere to come from (no download, no git)"));
        };
        let file = fetch(self, download_name, scratch)?;
        if !self.take.is_empty() {
            note!("taking {} file(s) out of {download_name}", self.take.len());
            let mut archive = open_zip(&file)?;
            // 📜 the last first, so the first (the one that matters) never arrives without the rest
            for (inside, to) in self.take.iter().rev() {
                let mut entry = archive
                    .by_name(inside)
                    .map_err(|error| format!("{inside} isn't in {download_name}: {error}"))?;
                write_whole(&mut entry, &folder.join(to))?;
            }
            return Ok(Some(path));
        }
        journal::tell(
            "unpack",
            &[("name", download_name), ("place", &folder.display().to_string())],
            &format!("unpacking {download_name} into {}", folder.display()),
        );
        // 🧱 built beside the real one, which only appears once it is whole
        let mut part = folder.as_os_str().to_os_string();
        part.push(".part");
        let part = PathBuf::from(part);
        let _ = fs::remove_dir_all(&part);
        let built = if download_name.ends_with(".zip") {
            unpack_all(&file, &part)
        } else if unpacked(download_name) {
            untar(&file, &part)
        } else {
            place(&file, &part.join(path.file_name().unwrap_or_default()), self.executable)
        };
        if let Err(problem) = built {
            let _ = fs::remove_dir_all(&part);
            return Err(problem);
        }
        if self.executable && unpacked(download_name) {
            let _ = executable(&part.join(path.strip_prefix(&folder).unwrap_or(&path)));
        }
        if folder.exists() {
            note!("--reinstall: replacing {}", folder.display());
            fs::remove_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        }
        fs::rename(&part, &folder).map_err(|error| {
            let _ = fs::remove_dir_all(&part);
            format!("couldn't make {}: {error}", folder.display())
        })?;
        Ok(Some(path))
    }
}

/// Something an install downloads into its artifacts, as it is there: `location.artifacts.uzdoom`.
pub struct Artifact {
    /// What it is called (its installation's name, else its field's).
    pub name: &'static str,
    /// The file that matters in its folder (`Installation::file`).
    pub path: PathBuf,
    /// Its folder in the artifacts.
    pub folder: PathBuf,
    /// Whether the install downloads it.
    pub on: bool,
    flag: &'static str,
    installation: &'static Installation,
    artifacts: PathBuf,
}

impl Artifact {
    /// `installation` (its field `flag`), as an artifact in `places` (`on`: `None` for always).
    pub fn new(
        flag: &'static str,
        installation: &'static Installation,
        places: &crate::Places,
        on: Option<bool>,
    ) -> Self {
        Artifact {
            name: installation.called(flag),
            path: installation.path(&places.artifacts, flag),
            folder: installation.folder(&places.artifacts, flag),
            on: on != Some(false),
            flag,
            installation,
            artifacts: places.artifacts.clone(),
        }
    }

    /// Its file, relative to the artifacts (`uzdoom/uzdoom.AppImage`), with `/` between folders
    /// on every system: for a TOML in the artifacts that names it.
    pub fn relative(&self) -> String {
        let relative = self.path.strip_prefix(&self.artifacts).unwrap_or(&self.path);
        relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Whether its file is there.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// The plan's line for it (`doc` is its field's doc comment).
    pub(crate) fn plan(&self, doc: &str) -> String {
        let installation = self.installation;
        let mut line = format!("  - {doc}");
        let what = [self.name, installation.version]
            .into_iter()
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let shown = self.folder.strip_prefix(&self.artifacts).unwrap_or(&self.folder).display();
        match (installation.download, installation.git) {
            (Some(_), _) if installation.blake3.is_some() || installation.sha256.is_some() => line.push_str(&format!(
                ": {what} is downloaded into {shown}/ in the artifacts, checked against its pinned size and hash"
            )),
            (Some(_), _) => line.push_str(&format!(": {what} is downloaded into {shown}/ in the artifacts")),
            (None, Some(git)) => line.push_str(&format!(": {what} is cloned from {git} into {shown}/ in the artifacts")),
            (None, None) => {}
        }
        line
    }

    /// Puts it in place, unless it is there already (and not `reinstall`). Returns its file, or
    /// `None` if nothing was done.
    pub(crate) fn install(&self, scratch: &Scratch, reinstall: bool) -> Outcome<Option<PathBuf>> {
        self.installation.install(self.flag, &self.artifacts, scratch, reinstall)
    }
}

/// Clones `git` into `folder`.
fn clone(git: &str, folder: &Path) -> Outcome<()> {
    if folder.exists() {
        note!("--reinstall: replacing {}", folder.display());
        fs::remove_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    let status = journal::run(
        std::process::Command::new("git")
            .args(["clone", "--depth", "1", git])
            .arg(folder),
        &format!("git clone {git}"),
    )
    .map_err(|error| format!("couldn't run git: {error}"))?;
    if !status.success() {
        return Err(format!("git clone {git} failed ({status})"));
    }
    Ok(())
}

/// Lets `file` run.
pub(crate) fn executable(file: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o755))?;
    }
    let _ = file;
    Ok(())
}

/// Copies `file` to `to`, by way of `<to>.part`, letting it run if it is `runs`.
pub(crate) fn place(file: &Path, to: &Path, runs: bool) -> Outcome<()> {
    let problem = |error: io::Error| format!("{}: {error}", to.display());
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(problem)?;
    }
    let mut part = to.as_os_str().to_os_string();
    part.push(".part");
    let part = PathBuf::from(part);
    fs::copy(file, &part).map_err(problem)?;
    if runs {
        executable(&part).map_err(problem)?;
    }
    // (a program that is running can be replaced this way)
    fs::rename(&part, to).map_err(problem)?;
    note!("copied {} to {}", file.display(), to.display());
    Ok(())
}

/// Downloads `installation` (whose download is called `name`) into the scratch folder, from its
/// `download` or else its `mirrors` in turn, checking its size and hashes on the way. Returns where
/// it is.
pub(crate) fn fetch(installation: &Installation, name: &str, scratch: &Scratch) -> Outcome<PathBuf> {
    let size = installation.size;
    let shown_size = if size > 0 { journal::size(size) } else { "its size unknown".into() };
    journal::tell(
        "download",
        &[("name", name), ("size", &shown_size)],
        &format!("Downloading {name} ({shown_size})"),
    );
    let mut last = format!("{name} has nowhere to be downloaded from");
    for url in installation.download.iter().chain(installation.mirrors) {
        match fetch_from(installation, url, name, scratch) {
            Ok(path) => return Ok(path),
            Err(problem) if problem.contains("isn't the file it should be") => return Err(problem),
            Err(problem) => {
                crate::warn!("{problem}");
                last = problem;
            }
        }
    }
    Err(last)
}

/// [`fetch`] from `url`.
fn fetch_from(installation: &Installation, url: &str, name: &str, scratch: &Scratch) -> Outcome<PathBuf> {
    use sha2::Digest;
    let size = installation.size;
    let path = scratch.dir()?.join(name);
    note!(
        "downloading {url} ({size} bytes, BLAKE3 {}, SHA-256 {}) to {}",
        installation.blake3.unwrap_or("unchecked"),
        installation.sha256.unwrap_or("unchecked"),
        path.display()
    );
    let failed = |error: &dyn std::fmt::Display| format!("downloading {url}: {error}");
    let mut reader = ureq::get(url)
        .call()
        .map_err(|error| failed(&error))?
        .into_body()
        .into_reader();
    let mut file = fs::File::create(&path).map_err(|error| failed(&error))?;
    let mut blake3 = blake3::Hasher::new();
    let mut sha256 = sha2::Sha256::new();
    let mut buf = vec![0; 256 * 1024];
    let mut total = 0;
    let mut shown = u64::MAX;
    loop {
        let read = match reader.read(&mut buf) {
            Ok(read) => read,
            Err(error) => {
                journal::progress_done();
                return Err(failed(&error));
            }
        };
        if read == 0 {
            break;
        }
        total += read as u64;
        if size > 0 && total > size {
            journal::progress_done();
            return Err(failed(&"it is bigger than it should be"));
        }
        blake3.update(&buf[..read]);
        sha256.update(&buf[..read]);
        file.write_all(&buf[..read]).map_err(|error| failed(&error))?;
        let progress = (total * 100).checked_div(size).unwrap_or(total / 1_000_000);
        if progress != shown {
            shown = progress;
            journal::progress(&if size > 0 {
                format!("{name}: {progress:>3}% of {}", journal::size(size))
            } else {
                format!("{name}: {progress} MB")
            });
        }
    }
    journal::progress_done();
    file.sync_all().map_err(|error| failed(&error))?;
    if size > 0 && total != size {
        return Err(failed(&"it ended early"));
    }
    let blake3 = blake3.finalize().to_hex().to_string();
    let sha256: String = sha256.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    let wrong = [(installation.blake3, &blake3, "BLAKE3"), (installation.sha256, &sha256, "SHA-256")]
        .into_iter()
        .find(|(pinned, hash, _)| pinned.is_some_and(|pinned| pinned != hash.as_str()));
    if let Some((_, hash, kind)) = wrong {
        note!("{name} came with {kind} {hash}");
        journal::tell(
            "corrupted",
            &[("name", name)],
            &format!("{name} isn't the file it should be"),
        );
        return Err(failed(&"it isn't the file it should be (its hash differs)"));
    }
    if installation.blake3.is_some() || installation.sha256.is_some() {
        note!("{name}: {total} bytes, and its hash is the one it should be");
        journal::tell("verified", &[("name", name)], &format!("{name}: checked"));
    } else {
        note!("{name}: {total} bytes, BLAKE3 {blake3} (unchecked)");
        journal::tell(
            "downloaded",
            &[("name", name), ("size", &journal::size(total))],
            &format!("{name}: downloaded"),
        );
    }
    Ok(path)
}

/// Whether a download called `name` is unpacked (a zip or a tarball) rather than kept.
fn unpacked(name: &str) -> bool {
    [".zip", ".tar.xz", ".tar.gz", ".tgz", ".tar"]
        .iter()
        .any(|ending| name.ends_with(ending))
}

/// Unpacks the tarball at `file` into `folder` with the system's `tar` (GNU tar, or the bsdtar
/// Windows has), which tells its compression itself.
fn untar(file: &Path, folder: &Path) -> Outcome<()> {
    fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(file)
        .arg("-C")
        .arg(folder)
        .status()
        .map_err(|error| format!("couldn't run tar, which unpacks {}: {error}", file.display()))?;
    if !status.success() {
        return Err(format!("tar couldn't unpack {} ({status})", file.display()));
    }
    Ok(())
}

fn open_zip(file: &Path) -> Outcome<zip::ZipArchive<fs::File>> {
    let opened = fs::File::open(file).map_err(|error| format!("{}: {error}", file.display()))?;
    zip::ZipArchive::new(opened).map_err(|error| format!("{}: {error}", file.display()))
}

/// Unpacks every file in the zip at `file` into `folder`. A name that would land outside it is
/// refused.
fn unpack_all(file: &Path, folder: &Path) -> Outcome<()> {
    let mut archive = open_zip(file)?;
    fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        let Some(name) = entry.enclosed_name() else {
            return Err(format!("{} has a file named {:?}", file.display(), entry.name()));
        };
        let to = folder.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&to).map_err(|error| format!("{}: {error}", to.display()))?;
        } else {
            write_whole(&mut entry, &to)?;
        }
    }
    Ok(())
}

/// Writes all of `from` to `to`, by way of `<to>.part`, so a file cut short is never left as if it
/// were whole.
fn write_whole(from: &mut impl Read, to: &Path) -> Outcome<()> {
    let problem = |error: io::Error| format!("{}: {error}", to.display());
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(problem)?;
    }
    let mut part = to.as_os_str().to_os_string();
    part.push(".part");
    let part = PathBuf::from(part);
    let written = (|| {
        let mut file = fs::File::create(&part)?;
        let bytes = io::copy(from, &mut file)?;
        file.sync_all()?;
        fs::rename(&part, to)?;
        Ok::<u64, io::Error>(bytes)
    })();
    match written {
        Ok(bytes) => note!("wrote {} ({bytes} bytes)", to.display()),
        Err(error) => {
            let _ = fs::remove_file(&part);
            return Err(problem(error));
        }
    }
    Ok(())
}

/// A folder for the downloads, in the system's temporary folder rather than beside the install,
/// made the first time it is needed and deleted with everything in it at the end.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Self {
        Self(std::env::temp_dir().join(format!("installation-groans-{}", std::process::id())))
    }

    pub(crate) fn dir(&self) -> Outcome<&Path> {
        fs::create_dir_all(&self.0)
            .map_err(|error| format!("couldn't make {}: {error}", self.0.display()))?;
        Ok(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
