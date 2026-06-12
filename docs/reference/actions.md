# Actions Reference

Every action you can bind in `[keys]`. Reference an action by its **string name**
(e.g. `"focus_left"`). See [Keybindings](/guide/keybindings) for the default map
and key syntax.

Actions marked **repeatable** keep sticky-leader mode active after firing (so you
can, e.g., hold the leader and tap `l` `l` `l`); all others return to normal
mode.

## Layout — panes & columns

| Action | Aliases | Repeat | Description |
| ------ | ------- | :----: | ----------- |
| `new_column_right` | | | New column to the right. |
| `new_row_below` | `new_workspace_below`, `split_down` | | New workspace below, with a pane. |
| `new_tile_below` | `stack_pane` | | New stacked tile in the active column. |
| `close_pane` | | | Close the focused pane. |
| `focus_left` | | ✓ | Focus the column to the left. |
| `focus_right` | | ✓ | Focus the column to the right. |
| `focus_up` | | ✓ | Focus the workspace above. |
| `focus_down` | | ✓ | Focus the workspace below. |
| `move_pane_left` | | ✓ | Move the column left within the workspace. |
| `move_pane_right` | | ✓ | Move the column right within the workspace. |
| `consume_into_column` | | | Pull the right neighbor into the column as a tile. |
| `expel_from_column` | | | Push the active tile out into a new column. |

## Layout — sizing

| Action | Repeat | Description |
| ------ | :----: | ----------- |
| `column_width_increase` | ✓ | Increase the column's width proportion. |
| `column_width_decrease` | ✓ | Decrease the column's width proportion. |
| `column_width_one_third` | ✓ | Set column to ⅓ width. |
| `column_width_half` | ✓ | Set column to ½ width. |
| `column_width_two_thirds` | ✓ | Set column to ⅔ width. |
| `column_width_full` | ✓ | Set column to full width. |
| `cycle_preset_width` | ✓ | Cycle forward through [`layout.preset_widths`](/reference/configuration#layout). |
| `cycle_preset_width_reverse` | ✓ | Cycle backward through preset widths. |
| `equalize_adjacent_columns` | ✓ | Equalize the column and its right neighbor. |
| `tile_height_increase` | ✓ | Grow the active tile (shrinking its neighbor). |
| `tile_height_decrease` | ✓ | Shrink the active tile (growing its neighbor). |

## Scrolling

All scrolling actions are **repeatable**.

| Action | Aliases | Description |
| ------ | ------- | ----------- |
| `scroll_line_up` / `scroll_line_down` | | Scroll one line. |
| `scroll_half_page_up` / `scroll_half_page_down` | | Scroll half a page. |
| `scroll_page_up` / `scroll_page_down` | | Scroll a full page. |
| `scroll_top` / `scroll_bottom` | | Jump to top / bottom of scrollback. |
| `prev_prompt` | `previous_prompt` | Jump to the previous OSC 133 prompt¹. |
| `next_prompt` | | Jump to the next OSC 133 prompt¹. |

¹ Requires [shell integration](/guide/shell-integration); no-ops without prompt
marks.

## Workspaces & sessions

| Action | Aliases | Repeat | Description |
| ------ | ------- | :----: | ----------- |
| `switch_workspace_N` | | ✓ | Switch to workspace index `N` (`0`–`8`), e.g. `switch_workspace_3`. |
| `next_session` | | | Switch to the next connection. |
| `prev_session` | `previous_session` | | Switch to the previous connection. |
| `new_session` | | | Create and switch to a new session. |
| `detach` | | | Detach the client (server keeps running). |

## Modes & overlays

| Action | Aliases | Description |
| ------ | ------- | ----------- |
| `enter_mode:NAME` | | Enter a named key table, e.g. `enter_mode:resize`. |
| `toggle_overview` | | Toggle overview. |
| `exit_overview` | | Exit overview. |
| `toggle_command_palette` | | Toggle the command palette. |
| `toggle_session_palette` | | Toggle the session / remote-host palette. |
| `toggle_settings` | `settings` | Toggle the settings panel. |
| `toggle_help` | `help` | Toggle the keybindings help overlay. |
| `toggle_broadcast` | | Toggle broadcast input to all panes. |
| `toggle_lock` | | Toggle lock (pass all keys to the terminal). |

## Search

| Action | Description |
| ------ | ----------- |
| `open_search` | Open the search bar. |
| `close_search` | Close search and restore scroll. |
| `search_next_match` | Jump to the next match. |
| `search_prev_match` | Jump to the previous match. |

## Command palette

| Action | Description |
| ------ | ----------- |
| `close_command_palette` | Close the palette. |
| `palette_up` / `palette_down` | Move the selection. |
| `palette_confirm` | Run the selected entry. |

## Clipboard

| Action | Description |
| ------ | ----------- |
| `clipboard_copy` | Copy the selection. |
| `clipboard_paste` | Paste from the clipboard. |

## Paste confirmation

| Action | Description |
| ------ | ----------- |
| `confirm_paste` | Confirm a pending paste. |
| `dismiss_paste_confirm` | Cancel the paste. |

## Text input (search / palette buffers)

| Action | Description |
| ------ | ----------- |
| `text_input` | Forward the typed character to the active input. |
| `text_backspace` | Delete the last character. |

## Key tables (advanced)

| Action | Description |
| ------ | ----------- |
| `activate_key_table:NAME` | Push a named key table onto the stack. |
| `deactivate_key_table` | Pop the current key table. |

::: tip Command palette
Most of these actions are also runnable by name from the **command palette**
(`Alt p`) — handy for actions you haven't bound to a key.
:::
