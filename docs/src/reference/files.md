# Files and folders

## Configuration folder

Tonique saves your settings, sample folders and themes in a configuration
folder. Where it is depends on your system:

| System  | Folder                                                   |
| ------- | -------------------------------------------------------- |
| Linux   | `~/.config/tonique/`                                     |
| macOS   | `~/Library/Application Support/com.Bytenosis.Tonique/`   |
| Windows | `%APPDATA%\Bytenosis\Tonique\config\`                    |

The folder contains:

| File or folder  | Contents                                                       |
| --------------- | -------------------------------------------------------------- |
| `settings.json` | Your settings: interface scale, audio device, engine, theme, and the shortcuts you changed. |
| `config.json`   | The sample folders listed in the browser.                      |
| `recent.json`   | The recent projects list.                                      |
| `themes/`       | The themes you created, one `.toml` file each.                 |

To reset all settings, close Tonique and delete `settings.json`. Settings
that are missing or that Tonique can't read are reset to their defaults,
and your other settings are kept.

## Project files

| Extension  | Contents                                                        |
| ---------- | --------------------------------------------------------------- |
| `.tonique` | A project. It's a JSON file that points to the audio files the project uses. See [Saving projects](../guide/projects.md). |

## Audio formats

| Format | Extension |
| ------ | --------- |
| WAV    | `.wav`    |
| MP3    | `.mp3`    |
| Ogg    | `.ogg`    |
