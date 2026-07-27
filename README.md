<p align="center">
  <img src="public/Tab%20logo.png" alt="WTFIS logo" width="180" />
</p>

<h1 align="center">Find your fucking Folder.</h1>

<p align="center">A local-first, inline terminal finder for getting back to the folder you meant.</p>

`wtfis` finds folders from a name, path, or typo. `cdd` is its short alias. Pick a match, press Enter, and your shell moves there.

## See It Work

### Rich TUI

<img src="public/TUI%20showcase.png" alt="WTFIS terminal interface listing matched directories" width="720" />

Search locally, inspect the matches, and choose the exact folder without leaving your terminal.

### Built-in Commands

<img src="public/COmmands%20showcase.png" alt="WTFIS command shortcuts for finding directories" width="720" />

Use `cdd` to find a folder, then attach a command for WTFIS to run after `cd`.

### Productivity Shortcuts

<img src="public/productivity-commands.png" alt="WTFIS productivity commands for navigating folder history" width="720" />

Use `--up`, `--prev`, and `--recent` instead of retracing your path.

### Rich Settings

<img src="public/Deep%20Settings.png" alt="WTFIS settings for configuring directory discovery" width="720" />

Choose search roots, depth, and the commands WTFIS can run after selecting a folder.

## Install

### macOS

Install with Homebrew, then source the wrapper once so WTFIS can change your current shell directory.

```bash
brew tap prophesourvolodymyr/wtfis
brew install wtfis

# Zsh
cat "$(brew --prefix)/share/wtfis/wtfis.zsh" >> ~/.zshrc
source ~/.zshrc
```

For Bash, source `wtfis.bash` from the same Homebrew directory in `~/.bashrc`.

### Linux

Download the `wtfis-linux-x86_64.tar.gz` asset from the [latest release](https://github.com/prophesourvolodymyr/WTFIS-CLI/releases/latest), extract it, and install the binaries plus shell wrapper:

```bash
tar -xzf wtfis-linux-x86_64.tar.gz
mkdir -p ~/.local/bin ~/.local/share/wtfis
install -m 755 wtfis-linux-x86_64/wtfis wtfis-linux-x86_64/cdd ~/.local/bin/
cp -R wtfis-linux-x86_64/shell ~/.local/share/wtfis

# Bash
echo 'source "$HOME/.local/share/wtfis/wtfis.bash"' >> ~/.bashrc
source ~/.bashrc
```

For Zsh, source `~/.local/share/wtfis/wtfis.zsh` from `~/.zshrc` instead.

### Windows

Download `wtfis-windows-x86_64.zip` from the [latest release](https://github.com/prophesourvolodymyr/WTFIS-CLI/releases/latest). In PowerShell, extract it, add the binary directory to the current session, and load the wrapper:

```powershell
Expand-Archive .\wtfis-windows-x86_64.zip -DestinationPath $env:LOCALAPPDATA\WTFIS -Force
$install = "$env:LOCALAPPDATA\WTFIS\wtfis-windows-x86_64"
$env:Path += ";$install"
. "$install\shell\wtfis.ps1"
```

To load the wrapper in future PowerShell sessions, add `. "$install\shell\wtfis.ps1"` to your `$PROFILE`.

## Use

```bash
wtfis                    # open inline search
wtfis my-project         # search immediately
cdd my-project           # short alias
wtfis --set              # configure search roots
wtfis --up               # recover a failed cd with a global search
wtfis --prev             # return to the previous directory
wtfis --root             # go to the detected project root
wtfis --last             # return to the last selected project
wtfis --where            # print the detected project root
wtfis --home             # go to your home directory
wtfis --recent           # open recent projects in the selector
wtfis /opencode          # cd to the selected project and run opencode
wtfis my-project /opencode # search a project, then run a command
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
cargo run -- my-project
```

## License

WTFPL
