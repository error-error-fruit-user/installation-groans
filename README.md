# Installation Groans

> **AI notice:** this project is developed with the help of AI. Much of the code and documentation was written with [Claude](https://claude.ai) (Anthropic), working under my direction and review. Commits that Claude helped write carry a `Co-Authored-By: Claude` trailer, so you can see which ones in the history.

The voices the installers of these projects speak in while they work: what they say when they begin, download, check, unpack, copy, run something, fail and finish. Each is a TOML of phrases per event, one picked at random each time:

- [`toiling-through-hell.toml`](toiling-through-hell.toml): **Toiling Through Hell**, the install as your computer's descent through hell, dragging every file up out of the pit. The default.
- [`computer-speak.toml`](computer-speak.toml): **Computer Speak**, a plain and formal line per step, for those who pass `--computer-speak`.

`tdl-install`, `nacht-install` and Nacht des Schreckens' `doomify.py` take them from a checkout of this repository beside theirs when there is one, or else from GitHub, and the installers build them in. The `{placeholders}` each event fills in are listed above it in the files. A new voice is a new TOML with the same events.

## License

[0BSD](LICENSE).
