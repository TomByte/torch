# Torch theme palettes

Torch offers four themes: Light, Dark, Kanagawa, and Ayu. Kanagawa uses Wave;
Ayu uses Mirage. The default Light and Dark palettes are unchanged.

- Kanagawa: [official Windows Terminal palette](https://github.com/rebelot/kanagawa.nvim/blob/master/extras/windows-terminal/kanagawa.json), Wave entry. Terminal background `#1F1F28`, foreground `#DCD7BA`, accent `#7E9CD8`, cursor `#C8C093`, and ANSI colors follow that palette. [MIT licence](theme-licenses/kanagawa.txt).
- Ayu: [official Ayu colors](https://github.com/ayu-theme/ayu-colors), npm `ayu@9.1.0`, generated Mirage palette from [mirage.yaml](https://github.com/ayu-theme/ayu-colors/blob/master/themes/mirage.yaml). Terminal background `#242936`, foreground `#CCCAC2`, accent/cursor `#FFCC66`, and ANSI colors follow Mirage. Bright black uses Mirage's UI gray `#707A8C` so CLI secondary text does not disappear against the dark terminal. [MIT licence](theme-licenses/ayu.txt).

Both themes use Torch's existing geometry and native controls. Sidebar surfaces
blend their foreground into their terminal background at 4% (top), 2% (bottom),
and 10% (selected row); secondary and muted text are opaque background/foreground
blends at 85% and 80%. Rules use an 18% foreground blend; hover and header rules
use foreground at 5% and 7% opacity. Attention uses each theme's bright-yellow
ANSI color, and the shadow retains Torch's dark shadow. The command surface
retains the existing 2% foreground blend. Text selection follows Torch's
existing theme engine rather than importing terminal-specific selection rules.

Retired built-in names remain readable for settings compatibility and resolve to
Light or Dark through the compatibility mapping, including preferences stored
for OS light and dark modes. They are absent from Torch's theme catalog and
picker. Existing custom theme files and references are retained; custom creation
and entries are hidden from Torch's four-choice picker and Settings.
