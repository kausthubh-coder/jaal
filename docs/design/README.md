# Design

`jaal.html` is the look of jaal. Open it in a browser. The switches at the top change the screen, the space, the theme and the platform.

| Screen | What it shows |
|---|---|
| Window | The main window at rest, the command bar, split view, the sidebar hidden, the tab menu, site controls, downloads, find, a new space and a permission prompt |
| Settings | General, Spaces and AI. The other five entries in the settings menu are not designed |
| Components | Every control at real size, the tab row states, and the colors, type sizes and corner sizes |

The page is a mock, not app code. The names and values of its CSS variables are the color tokens the real UI should use.

## Decisions

Made on 2026-11-09.

- The page floats as a card inside chrome tinted by the space's color. The address, favorites and spaces live in the sidebar. This beat a second direction with a spaces rail, a top address strip and an edge-to-edge page.
- A space sets two hues, one for the chrome and one for the accent. Every other color is computed from them, once for light and once for dark.
- A space shows as an icon, not a dot.
- On Windows the minimize, maximize and close buttons sit in the sidebar's top row, as in Zen. There is no strip above the page.
- Menus and popovers use the command bar's glass, one size down.

## Guesses, not decisions

- The tab menu's entries.
- The extension names in site controls ("Ad blocker", "Password manager") are stand-ins.
- The rows in Settings, AI.
- Favicons are letter tiles that stand in for real site icons.

## Not designed yet

| Area | Missing |
|---|---|
| First run | Import from Chrome or Arc, set as default browser, make the first space |
| jaal's own pages | History and archive, the full downloads list, extensions and the Web Store install prompt |
| Settings | Search, Privacy, Extensions, Shortcuts, About |
| On the page | Right-click menu for links and pages, error pages (offline, crashed tab, bad certificate), site dialogs, link preview, picture-in-picture |
| Motion | Drag to reorder, folder open and close, the sidebar sliding back, the change between spaces |
| Identity | App icon and wordmark |
| Later | AI panel, sync and sign-in |

## How it was made

Without the Critly skill, on purpose. A second thread designs jaal with Critly so the two can be compared.
