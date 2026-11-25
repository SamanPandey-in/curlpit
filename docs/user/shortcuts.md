# Keyboard shortcuts

These are all the key bindings in the app today, taken from `crates/app/src/main.rs` and the input handlers. On macOS, the modifier is `Cmd`. On Windows and Linux, it is `Ctrl`.

## Global

| Action | macOS | Windows / Linux |
|---|---|---|
| Send the request (press again to cancel it while it runs) | `Cmd+Enter` | `Ctrl+Enter` |
| Save the request (on the environments page: save the variables) | `Cmd+S` | `Ctrl+S` |
| New request tab | `Cmd+T` | `Ctrl+T` |
| Close the current tab | `Cmd+W` | `Ctrl+W` |
| Reopen the last closed tab | `Cmd+Shift+T` | `Ctrl+Shift+T` |
| Find in the response | `Cmd+F` | `Ctrl+F` |
| Show or hide the sidebar | `Cmd+B` | `Ctrl+B` |
| Command palette | `Cmd+K` or `Cmd+Shift+P` | `Ctrl+K` or `Ctrl+Shift+P` |

The tab shortcuts (new, close, reopen) and Find work only while the Requests view is showing, and Find needs a response to search. Closing a tab with unsaved changes needs a second press (the tab shows **Discard?**), and Reopen brings back its edits.

The global shortcuts need focus inside the window. If one doesn't fire right after you click a button, click into any text box first.

## In text boxes

| Where | Key | Does |
|---|---|---|
| URL box | `Enter` | Sends the request. Unlike `Cmd/Ctrl+Enter`, it never cancels one already running. |
| Find bar | `Enter` | Next match |
| Find bar | `Cmd+Enter` / `Ctrl+Enter` | Previous match |
| Body editor | `Cmd+F` / `Ctrl+F` | The editor's own find, while the editor has focus |

## Command palette

| Key | Does |
|---|---|
| Type | Fuzzy-filter commands by name or category ("theme dark", "env prod") |
| `Up` / `Down` | Move the highlight |
| `Enter` | Run the highlighted command |
| `Escape` | Close the palette (clicking outside it, or pressing the palette shortcut again, also closes it) |

The palette holds everything that has a shortcut, plus: go to Requests, Environments, Runner, Cookies or Settings; show Collections or History; Import file…; Import from clipboard; Export collection as Postman… or as JSON…; Theme: Light, Dark or Follow system; and one entry per environment to make it active.

## Not bound yet

- Shortcuts can't be customised yet.
- There is no request-diff shortcut (`Cmd/Ctrl+D`), because request diff isn't built yet.
