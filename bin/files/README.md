English | [简体中文](docs/i18n/zh-CN/README.md)

> This project is still in early development. Features and the interface will keep changing.
> **Linux + Wayland** is the primary support target. Support for X11, Windows, and macOS is still a work in progress.

# <img src="../../assets/files/icon-desktop.svg" alt="GDS Files icon" width="32" height="32" align="middle"> GDS Files

GDS Files is a native file manager built with GPUI, focused on making file browsing on Wayland beautiful and easy.

![Overview](docs/assets/overview.png)

## Key features

### Even a gorilla can use it!

A full file browser with a familiar Windows File Explorer feel. You don't need to be a tech person to get started.

![Browse folders, switch views, and filter files](docs/assets/file-browsing.gif)

### Whoa! A terminal, right here!

On Linux, the terminal might be even more essential than the file manager. So we've included a simple one to make life easier for power users.

![Terminal demo](docs/assets/terminal.png)

### Translucent windows that follow my system theme.

Now you can finally see your anime girl wallpaper. I know how important that is. You can also tweak the font and text size until everything feels just right.

![GDS Files follows the colors of 16 classic wallpapers in light mode](docs/assets/theme-sync.gif)

### Keyboard all the way!

Maybe you really don't have a mouse. We understand your predicament!

### And the absolute killer feature: **COPY PATH!!!**

Seriously, why do so many file managers still not support copying file paths? How is this even a thing?

![Copy a file path](docs/assets/copy-path.png)

## Install and launch

### Let your agent install it for you

```text
Install this file manager for me: https://github.com/GanDesktop/gds/blob/main/bin/files/README.md
```

### With a package manager

<!-- Package manager installation is not available yet. -->

### With Nix

**Install into your profile:**

```sh
nix profile install github:GanDesktop/gds#gds-files
```

**Install on NixOS:** Add this input to `flake.nix` and include `gds` in your `outputs` arguments:

```nix
inputs.gds.url = "github:GanDesktop/gds";
```

Add the following to your system's `modules` list:

```nix
gds.nixosModules.gds-files
gds.nixosModules.gds-portal
{
  programs.gds-files = {
    enable = true;             # Install Files and make it the default folder handler
    fileManager1.enable = true; # Handle requests such as "Show in Folder" from other apps
  };
  services.gds-portal.enable = true; # Install GDS Portal separately for system file dialogs
}
```

Rebuild from your system configuration directory:

```sh
sudo nixos-rebuild switch --flake .
```

## Settings and data

Open **Settings** from the main menu. Changes take effect immediately and are saved automatically.

On Linux, data lives in the following locations. If you set XDG environment variables, their directories are used instead:

| Data | Default location |
| --- | --- |
| Application settings | `~/.config/gds/files/settings.toml` |
| Favorites, recent locations, and view preferences for each location | `~/.local/state/gds/files/state.sqlite` |
| Standard thumbnail cache | `~/.cache/thumbnails/` |

## File picker

GDS Portal is GDS's desktop Portal backend. It currently provides the file chooser interface. Once GDS Portal is enabled, apps that use this interface can open Files' native picker, specifying open or save mode, file types, an initial location, and other options.

## Debug

Run the following command to enable in-memory logs and the log panel for this process. This feature is off by default:

```sh
gds-files --enable-logs
```

## License

GDS Files' own code is licensed under [Apache-2.0](../../LICENSE). Third-party dependencies keep their respective licenses.

## Development and integration

From the repository root, build the development version or launch it with logging enabled:

```sh
just build files
just dev files --enable-logs
```

See the [development guide](../../docs/development.md) for the development environment and build instructions.

You can also call the file picker from the command line when integrating it with another application:

```sh
gds-files --pick-file --multiple --filter 'Images:*.png,*.jpg,*.webp'
gds-files --pick-directory "$HOME/Documents"
```

On confirmation, the command-line picker writes JSON to standard output and exits. Canceling returns no selected files. See the [file picker documentation](../../docs/apps/files/picker.md) for arguments and the output format.
