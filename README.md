# WTFIS

**Where the fuck is** your project?

`wtfis` is a local-first, inline terminal project finder. `cdd` is its short alias. Type a project name, fix a typo, press Enter, and the shell changes into the selected directory.

## Install

```bash
brew tap prophesourvolodymyr/wtfis
brew install wtfis
```

Then add the shell integration once:

```bash
cat "$(brew --prefix)/share/wtfis/wtfis.zsh" >> ~/.zshrc
```

Restart your shell. For Bash, use `wtfis.bash` instead.

For PowerShell on Windows, dot-source `shell/wtfis.ps1` from your PowerShell profile.

## Use

```bash
wtfis                    # open inline search
wtfis Mascotify          # search immediately
cdd Mascotify            # short alias
wtfis --set              # configure search roots
wtfis --up               # recover a failed cd with a global search
wtfis --prev             # return to the previous directory
wtfis --root             # go to the detected project root
wtfis --last             # return to the last selected project
wtfis --where            # print the detected project root
wtfis --home             # go to your home directory
wtfis --recent           # open recent projects in the selector
wtfis /opencode          # cd to the selected project and run opencode
wtfis Mascotify /opencode # search a project, then run a command
wtfis --help             # open the inline command guide
```

`--prev`, `--root`, `--last`, and `--home` change directories directly without opening the selector. `--where` only prints the detected project root. `--recent` opens the recent-project selector.

On the first run, WTFIS shows a short inline introduction, then opens the setup wizard for search roots and preferences.

V1 uses local fuzzy matching and searches relevant project folders by default. Use `wtfis --set` to enable broader global recovery, add custom roots, or change depth. It does not upload paths or project data. Semantic search is planned for V2.

Launching `wtfis` with no query does not scan the filesystem. It opens immediately with up to five recently opened folders; scanning begins when you type the first search character.

Type `/` in the selector to browse command presets. Use `/add` to choose a project and attach a preset or custom command, `/exit` to cancel, or `/opencode` and similar commands to enter the project and run the command immediately. Configure presets with `wtfis --set`.

The inline guide from `wtfis --help` documents every available command and control. Inside the finder, use Up/Down and Enter to navigate, Escape to cancel, or click results with the mouse. Trackpad/mouse-wheel scrolling is intentionally ignored inside the inline UI. A unique confident fuzzy match opens directly; ambiguous matches stay in the selector.

The Rust core is cross-platform. Linux uses Bash/Zsh integration, while Windows uses the PowerShell wrapper to change the parent shell directory.

The configuration also supports `exact_depth` as a maximum directory depth, for example `roots = ["/Users/you/GSpace"]` with `exact_depth = 3` searches the root's first, second, and third layers. Shallower matches rank first, while exact names at deeper layers still match directly.

## Development

```bash
cargo test
cargo run -- Mascotify
```

## License

WTFPL
