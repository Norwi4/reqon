# Reqon selection patch

Upstream: egui 0.37.0, MIT / Apache-2.0.

- `Selection::preserve_text_color` keeps syntax-highlighted glyph vertices unchanged while adding the selection background. Defaults to false for upstream behavior.
- `Selection::extend_to_line_end` fills selected line breaks up to the multiline editor's content width. The last selected row still ends at the actual cursor. Defaults to false.
- Multiline `TextEdit` passes its content width to the shared selection painter. Selectable labels also preserve their foreground colors, without changing their layout.

Reqon enables both options in `src/theme.rs`. The regression test checks actual TextEdit rendering, blank lines, partial final lines, Unicode clipboard contents, and both themes.
