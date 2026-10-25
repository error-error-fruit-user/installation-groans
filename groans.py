"""groans.py: speaking in an installation-groans voice, for the Python install scripts of the
projects beside this one (publish.py, publish-install.py, appimage.py).

    import groans
    groans.start(journal_path, computer_speak=False)
    groans.section("build")
    groans.run(["cargo", "build", "--release"])
    groans.say("copy", f"Installed {target}", what=name, place=target)
    groans.finish("tdl")

Everything said goes through the voice: an event's line is one of its phrases (picked at random),
in its colour; a `--- Section ---` separator opens each part, in the voice's name for it; and
whatever another program prints (cargo, git, Blender...) is rewritten by the voice's [[rewrite]]
rules, or by its [fallback], so not a line comes through untouched. The journal keeps the plain
account. Dependency-free Python 3.8+ (3.11+ reads the voice's TOML; older ones speak plainly).

A script run by another (publish.py by publish-install.py) is told so
through the environment (GROANS_*): it speaks in the same voice and colours, writes into the same
journal rather than one of its own, says nothing the outer one says itself (its beginning, its end,
where the journal is, a section already open), and asks nothing, the outer one having asked.
"""

import os
import random
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_VOICE = "toiling-through-hell.toml"
COMPUTER_SPEAK = "computer-speak.toml"
GITHUB = "https://raw.githubusercontent.com/error-error-fruit-user/installation-groans/main/"

# What an outer script tells the ones it runs
ENV_JOURNAL = "GROANS_JOURNAL"      # the journal of the whole run
ENV_DEPTH = "GROANS_DEPTH"          # how many scripts deep this one runs: 0 on its own
ENV_VOICE = "GROANS_VOICE"          # the voice's file name
ENV_COLOR = "GROANS_COLOR"          # "1": colour what is printed, though it isn't a terminal
ENV_SECTION = "GROANS_SECTION"      # the section open when it was started
ENV_CAPTURED = "GROANS_CAPTURED"    # "1": the outer one journals what this prints itself
ENV_STARTED = "GROANS_STARTED"      # when the outermost script began (seconds since 1970)

# Said only by the outermost script: a script run by another opens with its own beginning (the outer
# one says nothing about starting it), but the end and where the journal is are the outer one's
OUTER_EVENTS = ("done", "journal")

voice = {}
journal = None
depth = 0
began = time.time()
open_section = None
colored = False
captured = False


def nested():
    """Whether another script runs this one."""
    return depth > 0


def load(name):
    """The voice `name` (a file name): from beside this file, else GitHub; {} if neither has it
    or this Python can't read TOML."""
    try:
        import tomllib
    except ImportError:
        return {}
    path = os.path.join(HERE, name)
    try:
        if os.path.isfile(path):
            with open(path, encoding="utf-8") as source:
                return tomllib.loads(source.read())
        import urllib.request
        with urllib.request.urlopen(GITHUB + name, timeout=5) as answer:
            return tomllib.loads(answer.read().decode())
    except Exception:
        return {}


def start(journal_path=None, computer_speak=False):
    """Begins speaking: in the outer script's voice and journal when another runs this one, else
    in Toiling Through Hell (or Computer Speak) with `journal_path` as the journal (None: none)."""
    global voice, journal, depth, open_section, colored, captured, began
    depth = int(os.environ.get(ENV_DEPTH, "0") or 0)
    # 🕰️ one clock for the whole run, so the journal's times go on from the outer script's
    try:
        began = float(os.environ.get(ENV_STARTED) or began)
    except ValueError:
        pass
    name = (os.environ.get(ENV_VOICE) if nested() else None) or (
        COMPUTER_SPEAK if computer_speak else DEFAULT_VOICE)
    voice = load(name)
    voice["file"] = name
    journal = os.environ.get(ENV_JOURNAL) or journal_path
    open_section = os.environ.get(ENV_SECTION) or None
    captured = os.environ.get(ENV_CAPTURED) == "1"
    no_color = "NO_COLOR" in os.environ
    colored = not no_color and (sys.stdout.isatty() or os.environ.get(ENV_COLOR) == "1")
    if os.name == "nt" and colored:
        os.system("")  # lets the Windows console show colours
    if journal and not nested():
        note(f"=== {os.path.basename(sys.argv[0])} {' '.join(sys.argv[1:])} ===")


def note(line):
    """Writes `line` down in the journal, with the time since the script began."""
    if not journal:
        return
    os.makedirs(os.path.dirname(os.path.abspath(journal)), exist_ok=True)
    with open(journal, "a", encoding="utf-8") as book:
        for part in str(line).splitlines() or [""]:
            book.write(f"[{time.time() - began:8.3f}s] {part}\n")


def paint(text, key):
    """`text` in the voice's colour for `key` (an event, `other`, `section`, `warning`)."""
    code = voice.get("colors", {}).get(key)
    return f"\033[{code}m{text}\033[0m" if colored and code else text


def show(text, key, plain=None, stream=None):
    """Prints `text` in `key`'s colour and journals it (with `plain` beside it, if different),
    unless the outer script journals what this prints."""
    print(paint(text, key), file=stream or sys.stdout, flush=True)
    if not captured:
        note(text if plain is None or plain == text else f"{text}  ({plain})")


def phrase(event, fills):
    """One of the voice's phrases for `event`, filled in; None without one."""
    phrases = voice.get("phrases", {}).get(event) or []
    if not phrases:
        return None
    said = random.choice(phrases)
    opens_with_name = said.startswith("{")
    for name, value in fills.items():
        said = said.replace("{" + name + "}", str(value))
    # 🔠 a sentence opens with a capital, but a name keeps its own (tdl, horde-hammer.pk3)
    return said if opens_with_name else said[:1].upper() + said[1:]


def interject():
    """Now and then something else speaks through the machine (the voice's [interjection])."""
    interjection = voice.get("interjection", {})
    if interjection.get("phrases") and random.random() < float(interjection.get("chance", 0)):
        colors = {"red": "1;31", "yellow": "1;33", "green": "1;32", "blue": "1;34",
                  "magenta": "1;35", "cyan": "1;36", "white": "1;37"}
        shout = random.choice(interjection["phrases"])
        code = colors.get(interjection.get("color", "red"), "1;31")
        print(f"\033[{code}m{shout}\033[0m" if colored else shout, flush=True)
        note(shout)


def say(event, plain, **fills):
    """Says `event` as one of the voice's phrases for it, or `plain` without one. Inside another
    script, its beginning, end and journal are left to the outer one (only written down)."""
    if nested() and event in OUTER_EVENTS:
        if not captured:
            note(plain)
        return
    said = phrase(event, fills)
    stream = sys.stderr if event == "problem" else sys.stdout
    if said is None:
        show(plain, event, stream=stream)
        return
    show(said, event, plain, stream)
    # a problem's phrase may leave the problem out: it is said plainly too
    if event == "problem" and str(fills.get("problem", plain)) not in said:
        show(plain, "other", stream=stream)
    interject()


def section(key):
    """Opens a part of the work with a `--- Section ---` separator, in the voice's name for it,
    unless that section is open already (the outer script opened it)."""
    global open_section
    if open_section == key:
        return
    open_section = key
    name = voice.get("sections", {}).get(key, key.replace("-", " ").title())
    show(f"--- {name} ---", "section")


def warning(lines, width=100):
    """`lines` under a red `--- Warning ---`, what a script will do before it asks; run by another
    script, whose own warning said it already, only written down."""
    if nested():
        for line in lines:
            note(line)
        return
    import shutil
    import textwrap
    show("--- " + voice.get("sections", {}).get("warning", "Warning") + " ---", "warning")
    width = min(shutil.get_terminal_size().columns, width)
    for line in lines:
        note(line)
        hang = len(line) - len(line.lstrip(" -"))
        print(textwrap.fill(line, width, subsequent_indent=" " * hang, break_on_hyphens=False,
                            break_long_words=False) if line else "")
    print()


def confirm(accept=False, flags=()):
    """The one question an install script asks, after its warning: "Are you sure? [y/n]". After
    the y the answer can carry any of the script's `flags`, the same ones its command line takes
    (`y --mode user --menu-entry`); the caller parses them after the command line's, so they win.
    Returns the flags added, or None for no. `accept` (--accept: the script's defaults) or being
    run by another script that asked already is yes, unasked."""
    if accept or nested():
        note("going ahead without asking" + (" (run by another script)" if nested() else " (--accept)"))
        return []
    if not sys.stdin.isatty():
        note("no one to ask, and no --accept: nothing was done")
        sys.exit("There is no one to ask, so nothing was done: pass --accept to go ahead.")
    question = phrase("confirm", {}) or "Are you sure? [y/n]"
    while True:
        answer = input(paint(question, "confirm") + " ").strip()
        note(f"asked {question!r}, the answer was {answer!r}")
        words = answer.split()
        if not words or words[0].lower() in ("n", "no"):
            return None
        if words[0].lower() not in ("y", "yes"):
            show(phrase("unclear", {}) or "y or n; after a y, any of the flags above (y --flag ...)",
                 "unclear")
            continue
        unknown = [word for word in words[1:] if word.startswith("-")
                   and word.split("=")[0] not in flags]
        if unknown:
            show(phrase("unknown_flags", {"flags": " ".join(unknown)})
                 or f"Not one of this script's flags: {' '.join(unknown)}", "unknown_flags")
            continue
        return words[1:]


def declined():
    """The answer was no: says that nothing was done, and stops."""
    show(phrase("declined", {}) or "Nothing was done.", "declined")
    sys.exit(1)


def rewrite(line):
    """Another program's line, in the voice: its first [[rewrite]] rule whose `pattern` matches it
    (a regex; `{1}`... are its groups), else its [fallback]."""
    # (a rule whose phrase is empty swallows a line that only repeats what was said: written down,
    # not shown)
    for rule in voice.get("rewrite", []):
        match = re.search(rule.get("pattern", "(?!)"), line)
        if match and rule.get("phrases"):
            said = random.choice(rule["phrases"])
            for number, group in enumerate(match.groups(), 1):
                said = said.replace("{" + str(number) + "}", group or "")
            return said.replace("{line}", line.strip()), rule.get("color", "other")
    fallback = voice.get("fallback", {})
    return fallback.get("format", "{line}").replace("{line}", line.rstrip()), "other"


class Strain:
    """What sits under another program's lines on a terminal: its progress line being redrawn in
    place (a line it rewrites with carriage returns, like cargo's Building bar), or, while its lines
    pour out faster than the voice's [strain] `threshold` a second, a pulsing line with how hard the
    machine labours, kept beating by a ticker of its own between lines."""

    def __init__(self):
        import threading
        self.on_terminal = sys.stdout.isatty()
        self.config = voice.get("strain", {}) if self.on_terminal else {}
        self.times = []
        self.footer = ""          # what is drawn under the lines now
        self.progress = False     # the footer is the program's own progress line
        self.began = time.monotonic()
        self.lock = threading.Lock()
        self.running = True
        if self.config.get("frames"):
            threading.Thread(target=self.tick, daemon=True).start()

    def wipe(self):
        if self.footer:
            sys.stdout.write("\r\033[K")
            self.footer = ""

    def draw(self, text):
        if text:
            sys.stdout.write(text)
        sys.stdout.flush()
        self.footer = text

    def pulse(self):
        """The strain line, as it beats now; empty while the lines come slowly."""
        now = time.monotonic()
        self.times = [moment for moment in self.times if now - moment < 1.0]
        threshold = float(self.config.get("threshold", 0) or 0)
        frames = self.config.get("frames") or []
        if not threshold or not frames or len(self.times) < threshold:
            return ""
        width = int(self.config.get("width", 16))
        filled = min(width, int(len(self.times) / threshold * width / 2) + 1)
        beat = int((now - self.began) / 0.15)
        frame = frames[beat % len(frames)].replace("{bar}", "█" * filled + "░" * (width - filled))
        colors = self.config.get("colors") or ["31"]
        return f"\033[{colors[beat % len(colors)]}m{frame}\033[0m" if colored else frame

    def tick(self):
        while self.running:
            time.sleep(0.15)
            with self.lock:
                if self.running and self.footer and not self.progress:
                    self.wipe()
                    self.draw(self.pulse())

    def line(self, text, stream=None):
        """Prints `text` above the footer, then draws the footer again."""
        with self.lock:
            self.times.append(time.monotonic())
            self.wipe()
            self.progress = False
            print(text, file=stream or sys.stdout, flush=True)
            if self.on_terminal:
                self.draw(self.pulse())

    def redraw(self, text):
        """The program's progress line, drawn in place of the last one."""
        with self.lock:
            self.wipe()
            self.progress = True
            if self.on_terminal:
                self.draw(text)

    def clear(self):
        """Takes the footer away, once the program is done."""
        with self.lock:
            self.running = False
            self.wipe()
            self.progress = False
            sys.stdout.flush()


def outer_env(voiced):
    """The environment for a script or program this one runs."""
    env = dict(os.environ)
    env[ENV_DEPTH] = str(depth + 1)
    env[ENV_VOICE] = voice.get("file", DEFAULT_VOICE)
    if journal:
        env[ENV_JOURNAL] = os.path.abspath(journal)
    if colored:
        env[ENV_COLOR] = "1"
    if open_section:
        env[ENV_SECTION] = open_section
    env[ENV_CAPTURED] = "0" if voiced else "1"
    env[ENV_STARTED] = repr(began)
    if not voiced:
        # 📊 cargo draws its Building bar into a pipe too, and plainly, for the voice to redraw
        env.setdefault("CARGO_TERM_PROGRESS_WHEN", "always")
        env.setdefault("CARGO_TERM_PROGRESS_WIDTH", "80")
        env.setdefault("CARGO_TERM_COLOR", "never")
    return env


def run(command, cwd=None, voiced=False, quiet_run=False):
    """Runs `command` (in `cwd`). A voiced one (an installation-groaned script or program) speaks
    for itself, into the same journal, so this says nothing about it; anything else is announced and
    read as it comes, each line rewritten in the voice, a line it redraws in place (a progress bar)
    redrawn here too. Raises CalledProcessError if it fails."""
    shown = " ".join("python3" if part == sys.executable else str(part) for part in command)
    if voiced or quiet_run:
        note(f"+ {shown}" + (f"  (in {cwd})" if cwd else ""))
    else:
        say("run", f"+ {shown}" + (f"  (in {cwd})" if cwd else ""), command=shown)
    env = outer_env(voiced)
    if voiced:
        code = subprocess.run(command, cwd=cwd, env=env).returncode
    else:
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT)
        strain = Strain()
        pending = b""

        def finished_line(raw):
            # a line it redrew with carriage returns ends as its last drawing
            parts = raw.decode(errors="replace").split("\r")
            text = next((part for part in reversed(parts) if part.strip()), "")
            if text.strip():
                said, key = rewrite(text)
                if said:
                    strain.line(paint(said, key))
                note("  | " + text)

        while True:
            chunk = process.stdout.read1(4096) if hasattr(process.stdout, "read1") \
                else process.stdout.readline()
            if not chunk:
                break
            *lines, pending = (pending + chunk).split(b"\n")
            for line in lines:
                # cargo's progress bar ends with a carriage return before the next line: only
                # what comes after its last one is the line
                if b"\r" in line and not line.rstrip(b"\r").strip():
                    continue
                finished_line(line)
            # 🔄 a line being redrawn (a progress bar) or waiting for an answer (a prompt) is shown
            # as it stands
            if b"\r" in pending:
                parts = pending.decode(errors="replace").split("\r")
                drawing = next((part for part in reversed(parts) if part.strip()), "")
                if drawing.strip():
                    said, key = rewrite(drawing.rstrip())
                    strain.redraw(paint(said or drawing.rstrip(), key))
                pending = pending[pending.rfind(b"\r") + 1:]
            elif pending.rstrip().endswith((b":", b"?", b"]")):
                strain.redraw(pending.decode(errors="replace"))
        if pending.strip():
            finished_line(pending)
        strain.clear()
        code = process.wait()
    note(f"  ended with code {code}")
    if code:
        raise subprocess.CalledProcessError(code, command)


def finish(what, ok=True, error=None):
    """The end: done (or the problem), and where the journal is. Inside another script, only
    a problem is said."""
    if ok:
        say("done", "Done.", what=what)
    else:
        say("problem", f"{what} failed: {error}", problem=error)
    if journal:
        say("journal", f"What it did is in {journal}", place=journal)


def other(line, stream=None):
    """A line that is no event, said the way another program's lines are (rewritten, coloured)."""
    said, key = rewrite(line)
    if said:
        print(paint(said, key), file=stream or sys.stdout, flush=True)
    if not captured:
        note(line)


def capture(command, cwd=None):
    """Runs `command`, saying so, and returns what it prints on stdout (data, like cargo's JSON);
    what it says on stderr is rewritten in the voice. Raises CalledProcessError if it fails."""
    shown = " ".join("python3" if part == sys.executable else str(part) for part in command)
    say("run", f"+ {shown}", command=shown)
    result = subprocess.run(command, cwd=cwd, env=outer_env(False), stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE)
    for line in result.stderr.decode(errors="replace").splitlines():
        if line.strip():
            other(line, sys.stderr)
    note(f"  ended with code {result.returncode}")
    if result.returncode:
        raise subprocess.CalledProcessError(result.returncode, command)
    return result.stdout


def built(target, count=None):
    """Says a file is made: `target`, with how many files went into it and its size."""
    size = os.path.getsize(target) / (1024 * 1024)
    inside = f"{count} files, " if count is not None else ""
    say("copy", f"{target}: {inside}{size:.1f} MB",
        what=f"{os.path.basename(target)} ({inside}{size:.1f} MB)",
        place=os.path.dirname(target) or ".")


# The system's lock icon for every shortcut: the icon theme's on Linux, shell32.dll's padlock on Windows
LOCK_ICON = "system-lock-screen"
WINDOWS_LOCK_ICON = r"%SystemRoot%\System32\shell32.dll,47"


def desktop_folder():
    """The desktop: xdg-user-dir's, else ~/Desktop; None if there is none."""
    home = os.path.expanduser("~")
    try:
        found = subprocess.run(["xdg-user-dir", "DESKTOP"], capture_output=True,
                               text=True).stdout.strip()
    except OSError:
        found = ""
    found = found or os.path.join(home, "Desktop")
    return found if os.path.isdir(found) and os.path.abspath(found) != os.path.abspath(home) else None


def exec_quoted(arg):
    """`arg` as a desktop entry's Exec line wants it."""
    if arg and not any(c in arg for c in " \t\n\"'\\><~|&;$*?#()`"):
        return arg
    return '"' + "".join("\\" + c if c in '"`$\\' else c for c in arg) + '"'


def shortcut(title, file_id, command, comment, desktop=False, menu=False):
    """A shortcut called `title`, running `command` in a terminal with the system's lock icon: on
    the desktop and/or in the app menu (the Start menu on Windows), as asked."""
    if not (desktop or menu):
        return
    section("shortcuts")
    if os.name == "nt":
        def ps(text):
            return "'" + text.replace("'", "''") + "'"
        folders = ([ "[Environment]::GetFolderPath('Desktop')"] if desktop else []) + (
            ["[Environment]::GetFolderPath('Programs')"] if menu else [])
        script = (
            "$shell = New-Object -ComObject WScript.Shell\n"
            f"foreach ($folder in @({', '.join(folders)})) {{\n"
            f"  $link = $shell.CreateShortcut((Join-Path $folder {ps(title + '.lnk')}))\n"
            f"  $link.TargetPath = {ps(command[0])}\n"
            f"  $link.Arguments = {ps(subprocess.list2cmdline(command[1:]))}\n"
            f"  $link.WorkingDirectory = {ps(os.path.dirname(os.path.abspath(command[0])))}\n"
            f"  $link.Description = {ps(comment)}\n"
            f"  $link.IconLocation = {ps(WINDOWS_LOCK_ICON)}\n"
            "  $link.Save()\n"
            "}\n")
        run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script],
            quiet_run=True)
        say("shortcut", f"Shortcuts for {title} made", what=title, place="the Start menu and desktop")
        return
    exec_line = " ".join(exec_quoted(arg) for arg in command)
    entry = ("[Desktop Entry]\nType=Application\n"
             f"Name={title}\nComment={comment}\n"
             f"Exec={exec_line.replace(chr(92), chr(92) * 2).replace('%', '%%')}\n"
             f"Icon={LOCK_ICON}\nTerminal=true\nCategories=Game;\n")
    data = os.environ.get("XDG_DATA_HOME") or os.path.join(os.path.expanduser("~"), ".local", "share")
    places = ([desktop_folder()] if desktop else []) + (
        [os.path.join(data, "applications")] if menu else [])
    for place in places:
        if place is None:
            other("note: there is no desktop folder to put a shortcut in", sys.stderr)
            continue
        os.makedirs(place, exist_ok=True)
        path = os.path.join(place, file_id + ".desktop")
        with open(path, "w", encoding="utf-8") as file:
            file.write(entry)
        os.chmod(path, 0o755)
        # a desktop launches only the shortcuts it is told to trust
        if place != os.path.join(data, "applications") and shutil_which("gio"):
            subprocess.run(["gio", "set", path, "metadata::trusted", "true"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        note(f"made the shortcut {path}: {exec_line}")
        say("shortcut", f"Shortcut: {path}", what=title, place=path)


def shutil_which(name):
    import shutil
    return shutil.which(name)


def newest_source(checkout):
    """When the newest file the checkout tracks (by git) was changed; 0 if it can't tell."""
    listed = subprocess.run(["git", "-C", checkout, "ls-files", "-z"], capture_output=True)
    if listed.returncode != 0:
        return 0
    newest = 0
    for name in listed.stdout.decode(errors="replace").split("\0"):
        try:
            newest = max(newest, os.path.getmtime(os.path.join(checkout, name)))
        except OSError:
            pass
    return newest


def up_to_date(outputs, checkouts):
    """Whether every one of `outputs` (what a build makes) is there and newer than every file the
    `checkouts` track: a build that needn't run again (unless --reinstall)."""
    if not outputs or not all(os.path.isfile(path) for path in outputs):
        return False
    built = min(os.path.getmtime(path) for path in outputs)
    return all(built >= newest_source(checkout) for checkout in checkouts if os.path.isdir(checkout))


def same_file(a, b):
    """Whether `b` is there with the very bytes of `a`: a copy that needn't be made again."""
    import filecmp
    return os.path.isfile(b) and filecmp.cmp(a, b, shallow=False)
