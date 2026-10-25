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
- The question every install asks once, `confirm` (it always ends in `[y/n]`), with `unclear`, `unknown_flags` and `declined` for what follows a bad answer or a no.

Two modules speak them, so every installer and script does it the same way:

- [`groans.py`](groans.py), for the Python scripts (`publish.py`, `publish-install.py`, `appimage.py`): `start`, `say`, `section`, `warning`, `confirm`, `run` (another program, rewritten in the voice), `capture`, `built`, `shortcut` and `finish`.
- [`groans.rs`](groans.rs), built into the Rust installers (`install-tdl`, `install-nacht`) as their journal: the same, as `tell`, `section`, `warning`, `confirm`, `run`, `shortcut`, `finish`, and `say!`/`warn!`/`note!`.

A script run by another (publish.py by publish-install.py, install-tdl by install-nacht) learns it from the environment (`GROANS_JOURNAL`, `GROANS_DEPTH`, `GROANS_VOICE`, `GROANS_COLOR`, `GROANS_SECTION`, `GROANS_CAPTURED`): it speaks in the same voice and colours, writes into the same log, says nothing the outer one already says (its beginning and end, where the log is, its warning, a section already open), and asks nothing.

The projects that install things carry this repository as a git submodule, `installation-groans/`, and read it from there (else from a checkout beside them, else GitHub).

## License

[0BSD](LICENSE).
