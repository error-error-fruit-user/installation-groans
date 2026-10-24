//! groans.rs: speaking in an installation-groans voice, for the Rust installers of the projects
//! beside this one (tdl-install, nacht-install), which build it in as their `journal` module
//! (their `build.rs` copies it from their installation-groans submodule, with the voices).
//!
//! The journal is everything the installer does, with the time since it began. `say!` says a line
//! and writes it down, `warn!` says a problem, `note!` only writes a detail down. [`tell`] says an
//! event as one of the voice's phrases for it, in its colour; [`section`] opens a part of the work
//! with a yellow `--- Section ---`; [`run`] runs another program, rewriting what it prints in the
//! voice ([`rewrite`]: its `[[rewrite]]` rules, else its `[fallback]`), so no line comes through
//! untouched. [`warning`] and [`confirm`] are the one question an install asks.
//!
//! An installer run by another script (tdl-install by nacht-install, either by publish-install.py)
//! is told so through the environment (`GROANS_*`, see [`child`]): it speaks in the same voice and
//! colours, writes into the same journal instead of its own, leaves its beginning, its end and
//! where the journal is to the outer one, never opens a section already open, and asks nothing.
//! Needs the `toml` and `regex-lite` crates.

use std::{
    fs::{self, File},
    io::{BufRead, BufReader, IsTerminal, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// What an outer script tells the ones it runs (the same as groans.py's).
const ENV_JOURNAL: &str = "GROANS_JOURNAL";
const ENV_DEPTH: &str = "GROANS_DEPTH";
const ENV_VOICE: &str = "GROANS_VOICE";
const ENV_COLOR: &str = "GROANS_COLOR";
const ENV_SECTION: &str = "GROANS_SECTION";
const ENV_CAPTURED: &str = "GROANS_CAPTURED";
/// Said only by the outermost script.
const OUTER_EVENTS: [&str; 3] = ["begin", "done", "journal"];

struct Journal {
    file: Option<File>,
    path: Option<PathBuf>,
    /// Lines written before the log file was opened.
    waiting: Vec<String>,
    began: Instant,
}

fn journal() -> &'static Mutex<Journal> {
    static JOURNAL: OnceLock<Mutex<Journal>> = OnceLock::new();
    JOURNAL.get_or_init(|| {
        Mutex::new(Journal {
            file: None,
            path: None,
            waiting: Vec::new(),
            began: Instant::now(),
        })
    })
}

/// How this run speaks: the voice, its file name, and what the outer script told it.
struct Speaking {
    voice: toml::Table,
    file: String,
    depth: u32,
    colored: bool,
    captured: bool,
}

static SPEAKING: OnceLock<Speaking> = OnceLock::new();
static OPEN_SECTION: Mutex<Option<String>> = Mutex::new(None);

fn speaking() -> Option<&'static Speaking> {
    SPEAKING.get()
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// Whether another script runs this one.
pub fn nested() -> bool {
    speaking().is_some_and(|speaking| speaking.depth > 0)
}

/// Starts speaking: in the outer script's voice when another runs this one, else in Toiling
/// Through Hell (`toiling`), or Computer Speak (`computer`) with `computer_speak`. A voice that
/// can't be read (or is empty, when the build found none) leaves plain lines.
pub fn speak_in(toiling: &str, computer: &str, computer_speak: bool) {
    let depth = env(ENV_DEPTH)
        .and_then(|depth| depth.parse().ok())
        .unwrap_or(0);
    let file = match env(ENV_VOICE).filter(|_| depth > 0) {
        Some(file) => file,
        None if computer_speak => "computer-speak.toml".to_string(),
        None => "toiling-through-hell.toml".to_string(),
    };
    let source = if file == "computer-speak.toml" { computer } else { toiling };
    let voice = source.parse::<toml::Table>().unwrap_or_default();
    let colored = env("NO_COLOR").is_none()
        && (std::io::stdout().is_terminal() || env(ENV_COLOR).as_deref() == Some("1"));
    if let Ok(mut open) = OPEN_SECTION.lock() {
        *open = env(ENV_SECTION);
    }
    let _ = SPEAKING.set(Speaking {
        voice,
        file,
        depth,
        colored,
        captured: env(ENV_CAPTURED).as_deref() == Some("1"),
    });
}

/// Writes `line` down, with the time since the install began.
pub fn write(line: &str) {
    let Ok(mut journal) = journal().lock() else {
        return;
    };
    let line = format!("[{:8.3}s] {line}", journal.began.elapsed().as_secs_f64());
    match &mut journal.file {
        Some(file) => {
            let _ = writeln!(file, "{line}");
        }
        None => journal.waiting.push(line),
    }
}

/// Starts keeping the journal in `dir/name` (added to if it is there already), with what was
/// written so far, or, run by another script, in that script's journal. Returns where it is, or
/// `None` if it can't be written (the install goes on without it).
pub fn open(dir: &Path, name: &str) -> Option<PathBuf> {
    let path = env(ENV_JOURNAL)
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join(name));
    fs::create_dir_all(path.parent()?).ok()?;
    let mut file = File::options().create(true).append(true).open(&path).ok()?;
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .unwrap_or_default();
    let mut journal = journal().lock().ok()?;
    let _ = writeln!(
        file,
        "=== {} {}, started at {since} (seconds since 1970) ===",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    for line in journal.waiting.drain(..) {
        let _ = writeln!(file, "{line}");
    }
    journal.file = Some(file);
    journal.path = Some(path.clone());
    Some(path)
}

/// A number from the time and how many have been drawn: enough to pick phrases with.
fn draw() -> usize {
    use std::sync::atomic::{AtomicU64, Ordering};
    static DRAWN: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.subsec_nanos() as u64)
        .unwrap_or_default();
    let mut x = nanos ^ DRAWN.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x as usize
}

fn voice() -> Option<&'static toml::Table> {
    speaking().map(|speaking| &speaking.voice)
}

/// `text` in the voice's colour for `key` (an event, `other`, `section`, `warning`).
pub fn paint(text: &str, key: &str) -> String {
    let code = voice()
        .and_then(|voice| voice.get("colors"))
        .and_then(|colors| colors.get(key))
        .and_then(toml::Value::as_str);
    match code {
        Some(code) if speaking().is_some_and(|speaking| speaking.colored) => {
            format!("\x1b[{code}m{text}\x1b[0m")
        }
        _ => text.to_string(),
    }
}

/// Prints `text` in `key`'s colour (on stderr with `error`) and writes it down, with `plain`
/// beside it if different, unless the outer script writes down what this prints.
fn show(text: &str, key: &str, plain: Option<&str>, error: bool) {
    let painted = paint(text, key);
    if error {
        eprintln!("{painted}");
    } else {
        println!("{painted}");
    }
    if !speaking().is_some_and(|speaking| speaking.captured) {
        match plain {
            Some(plain) if plain != text => write(&format!("{text}  ({plain})")),
            _ => write(text),
        }
    }
}

/// One of the voice's phrases for `event`, picked at random, its `{placeholders}` filled in from
/// `fills`; `None` without a voice or a phrase for it.
pub fn phrase(event: &str, fills: &[(&str, &str)]) -> Option<String> {
    let phrases = voice()?.get("phrases")?.get(event)?.as_array()?;
    let phrases: Vec<&str> = phrases.iter().filter_map(toml::Value::as_str).collect();
    if phrases.is_empty() {
        return None;
    }
    let chosen = phrases[draw() % phrases.len()];
    let mut said = chosen.to_string();
    for (name, value) in fills {
        said = said.replace(&format!("{{{name}}}"), value);
    }
    // 🔠 a sentence opens with a capital, but a name keeps its own (tdl, horde-hammer.pk3)
    if chosen.starts_with('{') {
        return Some(said);
    }
    let mut letters = said.chars();
    Some(match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => said,
    })
}

/// Says `event` in the voice (filled from `fills`), or `plain` without one, and writes it down.
/// Run by another script, its beginning, end and journal are left to the outer one.
pub fn tell(event: &str, fills: &[(&str, &str)], plain: &str) {
    if nested() && OUTER_EVENTS.contains(&event) {
        write(plain);
        return;
    }
    match phrase(event, fills) {
        Some(said) => {
            show(&said, event, Some(plain), false);
            interject();
        }
        None => show(plain, event, None, false),
    }
}

/// Now and then (the voice's `[interjection]` `chance`), something else speaks through the machine:
/// one of its phrases, in its color, bold.
fn interject() {
    let Some(interjection) = voice().and_then(|voice| voice.get("interjection")) else {
        return;
    };
    let chance = interjection
        .get("chance")
        .and_then(toml::Value::as_float)
        .unwrap_or(0.0);
    if (draw() % 10_000) as f64 >= chance.clamp(0.0, 1.0) * 10_000.0 {
        return;
    }
    let phrases: Vec<&str> = interjection
        .get("phrases")
        .and_then(toml::Value::as_array)
        .map(|phrases| phrases.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default();
    if phrases.is_empty() {
        return;
    }
    let said = phrases[draw() % phrases.len()];
    let code = match interjection
        .get("color")
        .and_then(toml::Value::as_str)
        .unwrap_or("red")
    {
        "yellow" => 33,
        "green" => 32,
        "blue" => 34,
        "magenta" => 35,
        "cyan" => 36,
        "white" => 37,
        _ => 31,
    };
    if speaking().is_some_and(|speaking| speaking.colored) {
        println!("\x1b[1;{code}m{said}\x1b[0m");
    } else {
        println!("{said}");
    }
    write(said);
}

/// Says a problem in the voice's `problem` phrases (and as it is, if the phrase leaves it out), on
/// stderr, and writes it down.
pub fn wail(problem: &str) {
    match phrase("problem", &[("problem", problem)]) {
        Some(said) => {
            show(&said, "problem", Some(problem), true);
            if !said.contains(problem) {
                show(problem, "other", None, true);
            }
        }
        None => show(problem, "problem", None, true),
    }
}

/// Opens a part of the work with a `--- Section ---` separator, in the voice's name for it,
/// unless it is open already (the outer script opened it).
pub fn section(key: &str) {
    if let Ok(mut open) = OPEN_SECTION.lock() {
        if open.as_deref() == Some(key) {
            return;
        }
        *open = Some(key.to_string());
    }
    let name = voice()
        .and_then(|voice| voice.get("sections"))
        .and_then(|sections| sections.get(key))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            let mut letters = key.chars();
            letters
                .next()
                .map(|first| first.to_uppercase().chain(letters).collect())
                .unwrap_or_default()
        });
    show(&format!("--- {name} ---"), "section", None, false);
}

/// `lines` under a red `--- Warning ---`: what the install will do, before it asks.
pub fn warning(lines: &[String]) {
    // 🔁 run by another script, whose own warning said it already: only written down
    if nested() {
        for line in lines {
            write(line);
        }
        return;
    }
    let name = voice()
        .and_then(|voice| voice.get("sections"))
        .and_then(|sections| sections.get("warning"))
        .and_then(toml::Value::as_str)
        .unwrap_or("Warning");
    show(&format!("--- {name} ---"), "warning", None, false);
    for line in lines {
        println!("{line}");
        write(line);
    }
    println!();
}

/// The one question an install asks, after its warning: "Are you sure? [y/n]". After the y the
/// answer can carry any of the installer's `flags` (`y --mode user --menu-entry`), the same ones its
/// command line takes, which the caller parses after the command line's so they win. Returns the
/// flags added, or `None` for no. `accept` (--accept: the defaults) or being run by another script
/// that asked already is yes, unasked; with no one to ask and no --accept, it is no.
pub fn confirm(accept: bool, flags: &[&str]) -> Option<Vec<String>> {
    if accept || nested() {
        write(if nested() {
            "going ahead without asking (run by another script)"
        } else {
            "going ahead without asking (--accept)"
        });
        return Some(Vec::new());
    }
    if !std::io::stdin().is_terminal() {
        write("no one to ask, and no --accept: nothing was done");
        return None;
    }
    let question = phrase("confirm", &[]).unwrap_or_else(|| "Are you sure? [y/n]".into());
    loop {
        print!("{} ", paint(&question, "confirm"));
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() {
            return None;
        }
        write(&format!("asked {question:?}, the answer was {:?}", answer.trim()));
        let words: Vec<&str> = answer.split_whitespace().collect();
        match words.first().map(|word| word.to_lowercase()) {
            None => return None,
            Some(word) if word == "n" || word == "no" => return None,
            Some(word) if word == "y" || word == "yes" => {}
            Some(_) => {
                let hint = phrase("unclear", &[]).unwrap_or_else(|| {
                    "y or n; after a y, any of the flags above (y --flag ...)".into()
                });
                show(&hint, "unclear", None, false);
                continue;
            }
        }
        let unknown: Vec<&str> = words[1..]
            .iter()
            .filter(|word| {
                word.starts_with('-') && !flags.contains(&word.split('=').next().unwrap_or(word))
            })
            .copied()
            .collect();
        if !unknown.is_empty() {
            let unknown = unknown.join(" ");
            let hint = phrase("unknown_flags", &[("flags", &unknown)])
                .unwrap_or_else(|| format!("Not one of this installer's flags: {unknown}"));
            show(&hint, "unknown_flags", None, false);
            continue;
        }
        return Some(words[1..].iter().map(|word| word.to_string()).collect());
    }
}

/// The answer was no: says that nothing was done.
pub fn declined() {
    let said = phrase("declined", &[]).unwrap_or_else(|| "Nothing was done.".into());
    show(&said, "declined", None, false);
}

/// Another program's line, in the voice: its first `[[rewrite]]` rule whose `pattern` matches it
/// (`{1}`... are its groups, `{line}` the whole line), else its `[fallback]`; and the colour key.
pub fn rewrite(line: &str) -> (String, String) {
    let rules = voice()
        .and_then(|voice| voice.get("rewrite"))
        .and_then(toml::Value::as_array);
    for rule in rules.into_iter().flatten() {
        let Some(pattern) = rule.get("pattern").and_then(toml::Value::as_str) else {
            continue;
        };
        let Ok(pattern) = regex_lite::Regex::new(pattern) else {
            continue;
        };
        let Some(found) = pattern.captures(line) else {
            continue;
        };
        let phrases: Vec<&str> = rule
            .get("phrases")
            .and_then(toml::Value::as_array)
            .map(|phrases| phrases.iter().filter_map(toml::Value::as_str).collect())
            .unwrap_or_default();
        if phrases.is_empty() {
            continue;
        }
        let mut said = phrases[draw() % phrases.len()].to_string();
        for number in 1..found.len() {
            let group = found.get(number).map_or("", |group| group.as_str());
            said = said.replace(&format!("{{{number}}}"), group);
        }
        let said = said.replace("{line}", line.trim());
        let color = rule
            .get("color")
            .and_then(toml::Value::as_str)
            .unwrap_or("other");
        return (said, color.to_string());
    }
    let format = voice()
        .and_then(|voice| voice.get("fallback"))
        .and_then(|fallback| fallback.get("format"))
        .and_then(toml::Value::as_str)
        .unwrap_or("{line}");
    (format.replace("{line}", line.trim_end()), "other".into())
}

/// A line of the installer's own that is no event, said the way another program's are.
pub fn other(line: &str) {
    let (said, key) = rewrite(line);
    println!("{}", paint(&said, &key));
    if !speaking().is_some_and(|speaking| speaking.captured) {
        write(line);
    }
}

/// Tells `command` (a script or program this runs) what an outer script tells: the journal, the
/// voice, how deep it runs, the colours, the open section, and whether `voiced` (it speaks for
/// itself) or this writes down what it prints.
pub fn child(command: &mut Command, voiced: bool) {
    let Some(speaking) = speaking() else {
        return;
    };
    command.env(ENV_DEPTH, (speaking.depth + 1).to_string());
    command.env(ENV_VOICE, &speaking.file);
    if let Some(path) = journal().lock().ok().and_then(|journal| journal.path.clone()) {
        command.env(ENV_JOURNAL, path);
    }
    if speaking.colored {
        command.env(ENV_COLOR, "1");
    }
    if let Some(open) = OPEN_SECTION.lock().ok().and_then(|open| open.clone()) {
        command.env(ENV_SECTION, open);
    }
    command.env(ENV_CAPTURED, if voiced { "0" } else { "1" });
}

/// The pulsing line under another program's lines when they pour out faster than the voice's
/// `[strain]` `threshold` a second: how hard the machine labours, redrawn under each new line.
struct Strain {
    times: Vec<Instant>,
    shown: bool,
    began: Instant,
    on_terminal: bool,
}

impl Strain {
    fn new() -> Self {
        Strain {
            times: Vec::new(),
            shown: false,
            began: Instant::now(),
            on_terminal: std::io::stdout().is_terminal(),
        }
    }

    /// Prints `text` (on stderr with `error`), with the pulsing line under it while the lines come
    /// thick and fast.
    fn line(&mut self, text: &str, error: bool) {
        let now = Instant::now();
        self.times
            .retain(|moment| now.duration_since(*moment).as_secs_f64() < 1.0);
        self.times.push(now);
        let mut out = std::io::stdout();
        if self.shown {
            let _ = write!(out, "\r\x1b[K");
            let _ = out.flush();
        }
        if error {
            eprintln!("{text}");
        } else {
            println!("{text}");
        }
        let strain = voice().and_then(|voice| voice.get("strain"));
        let threshold = strain
            .and_then(|strain| strain.get("threshold"))
            .and_then(|value| value.as_integer().map(|n| n as f64).or(value.as_float()))
            .unwrap_or(0.0);
        let frames: Vec<&str> = strain
            .and_then(|strain| strain.get("frames"))
            .and_then(toml::Value::as_array)
            .map(|frames| frames.iter().filter_map(toml::Value::as_str).collect())
            .unwrap_or_default();
        if !self.on_terminal
            || threshold <= 0.0
            || frames.is_empty()
            || (self.times.len() as f64) < threshold
        {
            self.shown = false;
            return;
        }
        let width = strain
            .and_then(|strain| strain.get("width"))
            .and_then(toml::Value::as_integer)
            .unwrap_or(16)
            .max(1) as usize;
        let filled = width.min((self.times.len() as f64 / threshold * width as f64 / 2.0) as usize + 1);
        let beat = (now.duration_since(self.began).as_secs_f64() / 0.15) as usize;
        let bar = "█".repeat(filled) + &"░".repeat(width - filled);
        let frame = frames[beat % frames.len()].replace("{bar}", &bar);
        let colors: Vec<&str> = strain
            .and_then(|strain| strain.get("colors"))
            .and_then(toml::Value::as_array)
            .map(|colors| colors.iter().filter_map(toml::Value::as_str).collect())
            .unwrap_or_default();
        let colored = speaking().is_some_and(|speaking| speaking.colored);
        if colored && !colors.is_empty() {
            let _ = write!(out, "\x1b[{}m{frame}\x1b[0m", colors[beat % colors.len()]);
        } else {
            let _ = write!(out, "{frame}");
        }
        let _ = out.flush();
        self.shown = true;
    }

    /// Takes the pulsing line away, once the program is done.
    fn clear(&mut self) {
        if self.shown {
            let mut out = std::io::stdout();
            let _ = write!(out, "\r\x1b[K");
            let _ = out.flush();
            self.shown = false;
        }
    }
}

/// Runs `command` (shown as `shown`), saying so, with what it prints rewritten in the voice and
/// written down. Returns how it ended.
pub fn run(command: &mut Command, shown: &str) -> std::io::Result<ExitStatus> {
    tell("run", &[("command", shown)], &format!("+ {shown}"));
    child(command, false);
    let mut process = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // 🧵 stderr in a thread of its own, so neither stream can stall the other; both draw through
    // the one strain line
    let strain = std::sync::Arc::new(Mutex::new(Strain::new()));
    let errors = process.stderr.take().map(|stderr| {
        let strain = strain.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    let (said, key) = rewrite(&line);
                    if let Ok(mut strain) = strain.lock() {
                        strain.line(&paint(&said, &key), true);
                    }
                    write(&format!("  | {line}"));
                }
            }
        })
    });
    if let Some(stdout) = process.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if !line.trim().is_empty() {
                let (said, key) = rewrite(&line);
                if let Ok(mut strain) = strain.lock() {
                    strain.line(&paint(&said, &key), false);
                }
                write(&format!("  | {line}"));
            }
        }
    }
    if let Some(errors) = errors {
        let _ = errors.join();
    }
    if let Ok(mut strain) = strain.lock() {
        strain.clear();
    }
    let status = process.wait()?;
    write(&format!("  ended: {status}"));
    Ok(status)
}

/// The end: done, and where the journal is (both left to the outer script when this is run by
/// another).
pub fn finish(what: &str) {
    tell("done", &[("what", what)], "Done.");
    let path = journal().lock().ok().and_then(|journal| journal.path.clone());
    if let Some(path) = path {
        let place = path.display().to_string();
        tell(
            "journal",
            &[("place", &place)],
            &format!("Everything this install did is in {place}"),
        );
    }
}

/// A size, as a person reads it.
pub fn size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{} MB", bytes / 1_000_000)
    } else {
        format!("{} KB", bytes.div_ceil(1000))
    }
}

/// The system's lock icon for every shortcut: the icon theme's on Linux, shell32.dll's padlock on
/// Windows.
const LOCK_ICON: &str = "system-lock-screen";
const WINDOWS_LOCK_ICON: &str = r"%SystemRoot%\System32\shell32.dll,47";

/// `arg` as a desktop entry's Exec line wants it.
fn exec_quoted(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|c| " \t\n\"'\\><~|&;$*?#()`".contains(c)) {
        return arg.to_string();
    }
    let mut quoted = String::from('"');
    for c in arg.chars() {
        if "\"`$\\".contains(c) {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

/// The user who ran this with sudo (`SUDO_USER`, while this runs as root) and their home: whom an
/// install's songs, settings and shortcuts are for, rather than root.
pub fn sudo_user() -> Option<(String, PathBuf)> {
    if cfg!(windows) {
        return None;
    }
    let user = env("SUDO_USER")?;
    let id = Command::new("id").arg("-u").output().ok()?;
    if id.stdout.trim_ascii() != b"0" {
        return None;
    }
    let entry = Command::new("getent").args(["passwd", &user]).output().ok()?;
    let home = String::from_utf8_lossy(&entry.stdout)
        .trim()
        .split(':')
        .nth(5)?
        .to_string();
    (!home.is_empty()).then(|| (user, PathBuf::from(home)))
}

/// The home of whom this install is for: the sudo user's, else `HOME`.
pub fn home() -> Option<PathBuf> {
    sudo_user()
        .map(|(_, home)| home)
        .or_else(|| env("HOME").map(PathBuf::from))
}

/// Gives `path` (and everything in it) to the sudo user, if this runs for one, so what is made for
/// them as root stays theirs to change.
pub fn give_back(path: &Path) {
    if let Some((user, _)) = sudo_user() {
        let given = Command::new("chown").arg("-R").arg(&user).arg(path).status();
        write(&format!("  gave {} to {user}: {given:?}", path.display()));
    }
}

/// The desktop: xdg-user-dir's, else ~/Desktop; `None` if there is none.
fn desktop_folder() -> Option<PathBuf> {
    let home = home()?;
    if sudo_user().is_some() {
        let desktop = home.join("Desktop");
        return desktop.is_dir().then_some(desktop);
    }
    let found = Command::new("xdg-user-dir")
        .arg("DESKTOP")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|found| !found.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("Desktop"));
    (found.is_dir() && found != home).then_some(found)
}

/// A shortcut called `title` that runs `command` (the program, then its arguments) in a terminal,
/// with the system's lock icon: on the desktop and/or in the app menu (the Start menu on Windows).
pub fn shortcut(title: &str, id: &str, command: &[String], comment: &str, desktop: bool, menu: bool) {
    if !(desktop || menu) || command.is_empty() {
        return;
    }
    section("shortcuts");
    if cfg!(windows) {
        let ps = |text: &str| format!("'{}'", text.replace('\'', "''"));
        let folders: Vec<&str> = [
            (desktop, "[Environment]::GetFolderPath('Desktop')"),
            (menu, "[Environment]::GetFolderPath('Programs')"),
        ]
        .into_iter()
        .filter_map(|(wanted, folder)| wanted.then_some(folder))
        .collect();
        let arguments: Vec<String> = command[1..]
            .iter()
            .map(|arg| {
                if arg.contains(' ') {
                    format!("\"{arg}\"")
                } else {
                    arg.clone()
                }
            })
            .collect();
        let working = Path::new(&command[0])
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default();
        let script = format!(
            "$shell = New-Object -ComObject WScript.Shell\nforeach ($folder in @({})) {{\n  \
             $link = $shell.CreateShortcut((Join-Path $folder {}))\n  $link.TargetPath = {}\n  \
             $link.Arguments = {}\n  $link.WorkingDirectory = {}\n  $link.Description = {}\n  \
             $link.IconLocation = {}\n  $link.Save()\n}}\n",
            folders.join(", "),
            ps(&format!("{title}.lnk")),
            ps(&command[0]),
            ps(&arguments.join(" ")),
            ps(&working),
            ps(comment),
            ps(WINDOWS_LOCK_ICON),
        );
        let made = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .status();
        match made {
            Ok(status) if status.success() => tell(
                "shortcut",
                &[("what", title), ("place", "the Start menu and desktop")],
                &format!("Shortcuts for {title} made"),
            ),
            _ => wail(&format!("the shortcuts for {title} couldn't be made")),
        }
        return;
    }
    let exec = command
        .iter()
        .map(|arg| exec_quoted(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={title}\nComment={comment}\nExec={}\n\
         Icon={LOCK_ICON}\nTerminal=true\nCategories=Game;\n",
        exec.replace('\\', "\\\\").replace('%', "%%")
    );
    let data = env("XDG_DATA_HOME")
        .filter(|_| sudo_user().is_none())
        .map(PathBuf::from)
        .or_else(|| home().map(|home| home.join(".local").join("share")));
    let mut places: Vec<(Option<PathBuf>, bool)> = Vec::new();
    if desktop {
        places.push((desktop_folder(), true));
    }
    if menu {
        places.push((data.map(|data| data.join("applications")), false));
    }
    for (place, on_desktop) in places {
        let Some(place) = place else {
            other("note: there is no desktop folder to put a shortcut in");
            continue;
        };
        let path = place.join(format!("{id}.desktop"));
        let written = fs::create_dir_all(&place).and_then(|()| fs::write(&path, &entry));
        if let Err(error) = written {
            wail(&format!("{}: {error}", path.display()));
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
        }
        // 🖱️ a desktop launches only the shortcuts it is told to trust
        if on_desktop {
            let _ = Command::new("gio")
                .args(["set", &path.display().to_string(), "metadata::trusted", "true"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        give_back(&path);
        write(&format!("made the shortcut {}: {exec}", path.display()));
        let place = path.display().to_string();
        tell(
            "shortcut",
            &[("what", title), ("place", &place)],
            &format!("Shortcut: {place}"),
        );
    }
}

/// Says a line on the terminal and writes it down.
#[macro_export]
macro_rules! say {
    ($($words:tt)*) => {
        $crate::journal::other(&format!($($words)*))
    };
}

/// Says a problem on stderr and writes it down.
#[macro_export]
macro_rules! warn {
    ($($words:tt)*) => {
        $crate::journal::wail(&format!($($words)*))
    };
}

/// Only writes a detail down.
#[macro_export]
macro_rules! note {
    ($($words:tt)*) => {
        $crate::journal::write(&format!("  {}", format!($($words)*)))
    };
}

/// What earlier runs of an install finished, kept in `install.state` in its artifacts (a step per
/// line), so running it again picks up where it left off: a finished step is skipped (and its
/// results, like a song the user has since changed, left alone), unless `--reinstall` asks for
/// everything again.
pub struct Steps {
    path: PathBuf,
    finished: Vec<String>,
    reinstall: bool,
}

impl Steps {
    /// The steps finished in the install whose artifacts are `art`.
    pub fn open(art: &Path, reinstall: bool) -> Self {
        let path = art.join("install.state");
        let finished = fs::read_to_string(&path)
            .map(|text| text.lines().map(str::to_string).collect())
            .unwrap_or_default();
        Steps {
            path,
            finished,
            reinstall,
        }
    }

    /// Whether `step` was finished by an earlier run (never, with `--reinstall`).
    pub fn done(&self, step: &str) -> bool {
        !self.reinstall && self.finished.iter().any(|finished| finished == step)
    }

    /// The steps earlier runs finished, for the plan (none with `--reinstall`).
    pub fn earlier(&self) -> Vec<&str> {
        if self.reinstall {
            return Vec::new();
        }
        self.finished.iter().map(String::as_str).collect()
    }

    /// Records `step` as finished.
    pub fn finish(&mut self, step: &str) {
        if !self.finished.iter().any(|finished| finished == step) {
            self.finished.push(step.to_string());
        }
        if let Some(folder) = self.path.parent() {
            let _ = fs::create_dir_all(folder);
        }
        let _ = fs::write(&self.path, self.finished.join("\n") + "\n");
        write(&format!("  finished: {step}"));
    }

    /// Says that `step` (`what`, at `place`) is skipped, an earlier run having finished it.
    pub fn skip(&self, what: &str, place: &str) {
        tell(
            "skip",
            &[("what", what), ("place", place)],
            &format!("{what} is done already, at {place} (--reinstall does it again)"),
        );
    }
}
