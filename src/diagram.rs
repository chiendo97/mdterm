use crate::style::{Style, StyledSpan};
use crate::theme::Theme;
use crossterm::style::Color;
use std::collections::HashSet;

mod merman_image;

pub struct RenderedMermaidImage {
    pub key: String,
    pub png: Vec<u8>,
}

pub fn render_mermaid_image(
    code: &str,
    theme: &Theme,
    width_cols: usize,
) -> Result<RenderedMermaidImage, String> {
    merman_image::render(code, theme, width_cols)
}

pub(crate) struct CardDrawRow {
    pub key: String,
    pub value_text: String,
    pub value_color: Option<Color>,
    /// If true, the value area shows `──▶` instead of text.
    pub is_connector: bool,
}

/// Fit `label` into `width` columns, marking a cut with an ellipsis.
///
/// Some routes have a fixed amount of room for their label (a left-right
/// lane runs between two node columns), and a label that overran it used to
/// paint over the route's own corner and off the edge of the canvas.
///
/// Below three columns there is no room for a cut label that still says
/// anything: one character and an ellipsis reads as a different word, and a
/// bare ellipsis says only that something was dropped. The label is left out
/// altogether instead, which at least does not misname the edge.
fn fit_label(label: &str, width: usize) -> String {
    if label.chars().count() <= width {
        return label.to_string();
    }
    if width < 3 {
        return String::new();
    }
    label
        .chars()
        .take(width - 1)
        .chain(std::iter::once('\u{2026}'))
        .collect()
}

// ───── Canvas ─────

const CONN_UP: u8 = 1;
const CONN_DOWN: u8 = 2;
const CONN_LEFT: u8 = 4;
const CONN_RIGHT: u8 = 8;

fn junction_char(connects: u8) -> char {
    match connects {
        c if c == CONN_UP | CONN_DOWN => '│',
        c if c == CONN_LEFT | CONN_RIGHT => '─',
        c if c == CONN_DOWN | CONN_RIGHT => '┌',
        c if c == CONN_DOWN | CONN_LEFT => '┐',
        c if c == CONN_UP | CONN_RIGHT => '└',
        c if c == CONN_UP | CONN_LEFT => '┘',
        c if c == CONN_UP | CONN_DOWN | CONN_RIGHT => '├',
        c if c == CONN_UP | CONN_DOWN | CONN_LEFT => '┤',
        c if c == CONN_DOWN | CONN_LEFT | CONN_RIGHT => '┬',
        c if c == CONN_UP | CONN_LEFT | CONN_RIGHT => '┴',
        c if c == CONN_UP | CONN_DOWN | CONN_LEFT | CONN_RIGHT => '┼',
        c if c == CONN_UP => '│',
        c if c == CONN_DOWN => '│',
        c if c == CONN_LEFT => '─',
        c if c == CONN_RIGHT => '─',
        _ => '·',
    }
}

#[derive(Clone)]
struct CanvasCell {
    ch: char,
    fg: Option<Color>,
    bg: Option<Color>,
    is_node: bool,
    connects: u8,
    /// Set when the cell holds an edge's arrowhead, to the directions the
    /// edge itself runs through the cell.
    ///
    /// The head is the one cell of an edge that says which of the two nodes it
    /// joins is the destination, and it is a single cell, so a line drawn
    /// across it does not clutter the edge, it deletes the edge's direction
    /// outright. [`Canvas::add_connection`] therefore records a crossing here
    /// without repainting the glyph, and
    /// a line doing the crossing is drawn with a break in it.
    arrow_axis: Option<u8>,
}

impl Default for CanvasCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: None,
            bg: None,
            is_node: false,
            connects: 0,
            arrow_axis: None,
        }
    }
}

pub(crate) struct Canvas {
    width: usize,
    height: usize,
    cells: Vec<Vec<CanvasCell>>,
}

impl Canvas {
    pub(crate) fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![vec![CanvasCell::default(); width]; height],
        }
    }

    pub(crate) fn set(&mut self, x: usize, y: usize, ch: char, fg: Option<Color>) {
        if y < self.height && x < self.width {
            self.cells[y][x].ch = ch;
            self.cells[y][x].fg = fg;
            // Whatever was here has been replaced, so a cell that held an
            // arrowhead no longer does and must not go on being protected as
            // though it did.
            self.cells[y][x].arrow_axis = None;
        }
    }

    /// Write the arrowhead that ends a forward edge, marking the cell so that
    /// a later crossing cannot paint the head away.
    fn set_arrow(&mut self, x: usize, y: usize, ch: char, axis: u8, fg: Option<Color>) {
        self.set(x, y, ch, fg);
        if y < self.height && x < self.width {
            self.cells[y][x].arrow_axis = Some(axis);
        }
    }

    pub(crate) fn add_connection(&mut self, x: usize, y: usize, dir: u8, fg: Option<Color>) {
        if y < self.height && x < self.width {
            let cell = &mut self.cells[y][x];
            if cell.is_node {
                return;
            }
            cell.connects |= dir;
            // An arrowhead keeps its glyph and its colour. The direction is
            // still recorded, so a route drawn through here later joins up
            // with the crossing, but repainting the cell as a junction would
            // delete the only mark that says where the edge ends, leaving two
            // boxes joined by a line that points at neither.
            if cell.arrow_axis.is_none() {
                cell.ch = junction_char(cell.connects);
                if fg.is_some() {
                    cell.fg = fg;
                }
            }
        }
    }

    fn set_label(&mut self, x: usize, y: usize, ch: char, fg: Option<Color>) {
        self.set(x, y, ch, fg);
    }

    fn set_node_bg(&mut self, x: usize, y: usize, ch: char, fg: Option<Color>, bg: Option<Color>) {
        if y < self.height && x < self.width {
            self.cells[y][x].ch = ch;
            self.cells[y][x].fg = fg;
            self.cells[y][x].bg = bg;
            self.cells[y][x].is_node = true;
        }
    }

    /// Draw a multi-row card (table-like node) used by the JSON graph view.
    /// Returns the y-coordinate of each content row for edge routing.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_card(
        &mut self,
        left_x: usize,
        top_y: usize,
        width: usize,
        title: &str,
        rows: &[CardDrawRow],
        border_fg: Option<Color>,
        title_fg: Option<Color>,
        key_fg: Option<Color>,
        highlight_rows: &HashSet<usize>,
        highlight_fg: Option<Color>,
        card_highlight_bg: Option<Color>,
    ) -> Vec<usize> {
        if width < 4 {
            return Vec::new();
        }
        let inner = width - 2; // space between │ and │
        let bg = card_highlight_bg;

        // ── top border with title: ╭─ title ─────╮ ──
        self.set_node_bg(left_x, top_y, '╭', border_fg, bg);
        self.set_node_bg(left_x + 1, top_y, '─', border_fg, bg);
        let title_chars: Vec<char> = title.chars().collect();
        let max_title = inner.saturating_sub(3); // "─" + space on each side
        let show_title = title_chars.len().min(max_title);
        self.set_node_bg(left_x + 2, top_y, ' ', title_fg, bg);
        for (i, &ch) in title_chars[..show_title].iter().enumerate() {
            self.set_node_bg(left_x + 3 + i, top_y, ch, title_fg, bg);
        }
        let fill_start = left_x + 3 + show_title;
        self.set_node_bg(fill_start, top_y, ' ', border_fg, bg);
        for x in (fill_start + 1)..(left_x + width - 1) {
            self.set_node_bg(x, top_y, '─', border_fg, bg);
        }
        self.set_node_bg(left_x + width - 1, top_y, '╮', border_fg, bg);

        // ── content rows ──
        let key_col_width = rows
            .iter()
            .map(|r| r.key.chars().count())
            .max()
            .unwrap_or(0)
            .min(inner.saturating_sub(4));

        let mut row_ys = Vec::with_capacity(rows.len());
        for (ri, row) in rows.iter().enumerate() {
            let y = top_y + 1 + ri;
            row_ys.push(y);

            let is_highlight = highlight_rows.contains(&ri);
            let row_key_fg = if is_highlight { highlight_fg } else { key_fg };
            let row_val_fg = if is_highlight {
                highlight_fg
            } else {
                row.value_color
            };

            // left border
            self.set_node_bg(left_x, y, '│', border_fg, bg);

            // space after border
            self.set_node_bg(left_x + 1, y, ' ', row_key_fg, bg);

            // key text
            let key_chars: Vec<char> = row.key.chars().collect();
            let show_key = key_chars.len().min(key_col_width);
            for (i, &ch) in key_chars[..show_key].iter().enumerate() {
                self.set_node_bg(left_x + 2 + i, y, ch, row_key_fg, bg);
            }
            // pad key column
            for i in show_key..key_col_width {
                self.set_node_bg(left_x + 2 + i, y, ' ', row_key_fg, bg);
            }

            // gap between key and value
            let val_start = left_x + 2 + key_col_width + 1;
            self.set_node_bg(val_start - 1, y, ' ', row_val_fg, bg);

            // value text (fill remaining space)
            let val_space = (left_x + width - 1).saturating_sub(val_start + 1);
            if row.is_connector {
                // draw ──▶ at the right edge of the card
                for x in val_start..(left_x + width - 1) {
                    self.set_node_bg(x, y, ' ', row_val_fg, bg);
                }
                // put the arrow near the right border
                let arrow_start = (left_x + width - 1).saturating_sub(4);
                if arrow_start >= val_start {
                    self.set_node_bg(arrow_start, y, '─', row_val_fg, bg);
                    self.set_node_bg(arrow_start + 1, y, '─', row_val_fg, bg);
                    self.set_node_bg(arrow_start + 2, y, '▶', row_val_fg, bg);
                }
            } else {
                let val_chars: Vec<char> = row.value_text.chars().collect();
                let show_val = val_chars.len().min(val_space);
                for (i, &ch) in val_chars[..show_val].iter().enumerate() {
                    self.set_node_bg(val_start + i, y, ch, row_val_fg, bg);
                }
                // pad remaining
                for i in show_val..val_space {
                    self.set_node_bg(val_start + i, y, ' ', row_val_fg, bg);
                }
            }

            // space before right border
            self.set_node_bg(left_x + width - 2, y, ' ', border_fg, bg);
            // right border
            self.set_node_bg(left_x + width - 1, y, '│', border_fg, bg);
        }

        // ── bottom border: ╰─────────╯ ──
        let bot_y = top_y + 1 + rows.len();
        self.set_node_bg(left_x, bot_y, '╰', border_fg, bg);
        for x in (left_x + 1)..(left_x + width - 1) {
            self.set_node_bg(x, bot_y, '─', border_fg, bg);
        }
        self.set_node_bg(left_x + width - 1, bot_y, '╯', border_fg, bg);

        row_ys
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_edge_lr(
        &mut self,
        src_right_x: usize,
        src_cy: usize,
        dst_left_x: usize,
        dst_cy: usize,
        label: Option<&str>,
        edge_fg: Option<Color>,
        label_fg: Option<Color>,
        mid_x_override: Option<usize>,
        label_bounds: Option<(usize, usize)>,
    ) {
        if src_right_x + 1 >= dst_left_x {
            return;
        }

        let mid_x =
            mid_x_override.unwrap_or_else(|| src_right_x + 1 + (dst_left_x - src_right_x - 1) / 2);

        if src_cy == dst_cy {
            // Straight right
            for x in (src_right_x + 1)..dst_left_x {
                self.add_connection(x, src_cy, CONN_LEFT | CONN_RIGHT, edge_fg);
            }
            // Arrow replaces last segment
            self.set_arrow(dst_left_x - 1, dst_cy, '▶', CONN_LEFT | CONN_RIGHT, edge_fg);

            // Label above the horizontal line, held inside the free columns.
            if let Some(text) = label {
                let label_y = if src_cy > 0 { src_cy - 1 } else { 0 };
                let (label_x, room) = match label_bounds {
                    Some((lo, hi)) => {
                        let x = (src_right_x + 2).max(lo);
                        (x, (hi + 1).saturating_sub(x))
                    }
                    None => (src_right_x + 2, usize::MAX),
                };
                for (i, ch) in fit_label(text, room).chars().enumerate() {
                    self.set_label(label_x + i, label_y, ch, label_fg);
                }
            }
        } else {
            // Right from source to mid_x
            for x in (src_right_x + 1)..mid_x {
                self.add_connection(x, src_cy, CONN_LEFT | CONN_RIGHT, edge_fg);
            }

            // Junction at mid_x, source row
            let src_turn = if dst_cy > src_cy {
                CONN_LEFT | CONN_DOWN
            } else {
                CONN_LEFT | CONN_UP
            };
            self.add_connection(mid_x, src_cy, src_turn, edge_fg);

            // Vertical segment
            let (min_y, max_y) = if src_cy < dst_cy {
                (src_cy, dst_cy)
            } else {
                (dst_cy, src_cy)
            };
            for y in (min_y + 1)..max_y {
                self.add_connection(mid_x, y, CONN_UP | CONN_DOWN, edge_fg);
            }

            // Junction at mid_x, destination row
            let dst_turn = if dst_cy > src_cy {
                CONN_UP | CONN_RIGHT
            } else {
                CONN_DOWN | CONN_RIGHT
            };
            self.add_connection(mid_x, dst_cy, dst_turn, edge_fg);

            // Right from mid_x to destination
            for x in (mid_x + 1)..dst_left_x {
                self.add_connection(x, dst_cy, CONN_LEFT | CONN_RIGHT, edge_fg);
            }

            // Arrow
            self.set_arrow(dst_left_x - 1, dst_cy, '▶', CONN_LEFT | CONN_RIGHT, edge_fg);

            // Label beside the vertical segment: right of the bend where
            // there is room for it, otherwise left of the bend, and cut when
            // neither side can hold it. Running past the free columns would
            // hide a feedback route or the destination box.
            //
            // The edge's own two horizontal runs are what the label row has to
            // clear. One sits on `src_cy`, from the source out to the bend; the
            // other on `dst_cy`, from the bend in to the destination. Any row
            // strictly between them is clear of both, so that is where the
            // label goes. Rows one apart leave no such row, and there the label
            // takes whichever side of the bend the run on its own row does not
            // reach.
            if let Some(text) = label {
                let (label_y, allow_left, allow_right) = if max_y - min_y >= 2 {
                    ((min_y + (max_y - min_y - 1) / 2).max(min_y + 1), true, true)
                } else {
                    (min_y, min_y != src_cy, min_y != dst_cy)
                };
                let len = text.chars().count();
                let (label_x, room) = match label_bounds {
                    Some((lo, hi)) => {
                        let right_x = (mid_x + 2).max(lo);
                        let right_room = if allow_right {
                            (hi + 1).saturating_sub(right_x)
                        } else {
                            0
                        };
                        let left_hi = mid_x.saturating_sub(1).min(hi);
                        let left_room = if allow_left {
                            (left_hi + 1).saturating_sub(lo)
                        } else {
                            0
                        };
                        if len <= right_room {
                            (right_x, right_room)
                        } else if len <= left_room {
                            (left_hi + 1 - len, left_room)
                        } else if right_room >= left_room {
                            (right_x, right_room)
                        } else {
                            (lo, left_room)
                        }
                    }
                    None if allow_right => (mid_x + 2, usize::MAX),
                    None => (mid_x.saturating_sub(1 + len), len),
                };
                let fitted = fit_label(text, room);
                // Whichever side it took, the label has to clear this edge's
                // own two horizontal runs: the one on `src_cy` reaching out to
                // the bend, and the one on `dst_cy` coming back in from it. A
                // label written over either erases the edge it names.
                #[cfg(test)]
                if !fitted.is_empty() {
                    let clear = if label_x > mid_x {
                        label_y != dst_cy
                    } else {
                        label_y != src_cy
                    };
                    assert!(
                        clear,
                        "label {fitted:?} sits on the edge's own run at ({label_x}, {label_y})"
                    );
                }
                for (i, ch) in fitted.chars().enumerate() {
                    self.set_label(label_x + i, label_y, ch, label_fg);
                }
            }
        }
    }

    pub(crate) fn to_span_rows(&self, theme: &Theme) -> Vec<Vec<StyledSpan>> {
        let default_bg = Some(theme.code_bg);
        self.cells
            .iter()
            .map(|row| {
                let mut spans = Vec::new();
                let mut i = 0;
                while i < row.len() {
                    let fg = row[i].fg.unwrap_or(theme.fg);
                    let cell_bg = row[i].bg.or(default_bg);
                    let mut text = String::new();
                    let mut j = i;
                    while j < row.len()
                        && row[j].fg.unwrap_or(theme.fg) == fg
                        && row[j].bg.or(default_bg) == cell_bg
                    {
                        text.push(row[j].ch);
                        j += 1;
                    }
                    spans.push(StyledSpan {
                        text,
                        style: Style {
                            fg: Some(fg),
                            bg: cell_bg,
                            ..Default::default()
                        },
                    });
                    i = j;
                }
                spans
            })
            .collect()
    }
}
