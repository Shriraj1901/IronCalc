#![deny(missing_docs)]
//! A block of cells with everything a renderer needs to paint them, fetched
//! in one call rather than one call per cell.

use std::collections::HashMap;

use serde::Serialize;

use crate::{
    cf_types::ExtendedStyle,
    expressions::utils::{is_valid_column_number, is_valid_row},
};

use super::common::UserModel;

/// The formatted value of a non-empty cell in a [CellsBlock].
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct CellText {
    /// The row of the cell
    pub row: i32,
    /// The column of the cell
    pub column: i32,
    /// The value as [UserModel::get_formatted_cell_value] returns it
    pub text: String,
    /// The [crate::types::CellType] of the cell, as its numeric value
    pub cell_type: i32,
}

/// A rectangle of cells with their styles and texts: what a renderer needs
/// to paint them, fetched in one call. See [UserModel::get_cells_block].
#[derive(Serialize, Debug, Clone)]
pub struct CellsBlock {
    /// The first row of the block
    pub row: i32,
    /// The first column of the block
    pub column: i32,
    /// The number of columns
    pub width: i32,
    /// The number of rows
    pub height: i32,
    /// The distinct styles of the cells, each as
    /// [UserModel::get_extended_cell_style] returns it
    pub styles: Vec<ExtendedStyle>,
    /// For every cell of the block, in row-major order, the index in
    /// `styles` of its style
    pub style_indices: Vec<u32>,
    /// The cells with a non-empty formatted value, in row-major order
    pub texts: Vec<CellText>,
}

/// A cell beyond the edge of the sheet, or a neighbour without a border on
/// the side facing the cell: it leaves the cell's style as it is.
const NO_NEIGHBOUR: i32 = -1;

impl UserModel<'_> {
    /// Returns the `height` by `width` rectangle of cells whose top-left
    /// cell is (`row`, `column`), with the style of every cell and the text
    /// of the non-empty ones.
    ///
    /// A cell's style here is its extended style (see
    /// [UserModel::get_extended_cell_style]): its own style with the
    /// conditional formats applied and the borders of its neighbours
    /// merged in. Cells mostly share styles, so the block lists each
    /// distinct style once and gives every cell an index into that list.
    ///
    /// Fails if the sheet does not exist or the rectangle does not fit in
    /// the sheet.
    pub fn get_cells_block(
        &self,
        sheet: u32,
        row: i32,
        column: i32,
        width: i32,
        height: i32,
    ) -> Result<CellsBlock, String> {
        if width < 1 || height < 1 {
            return Err(format!("Invalid block size: {width}x{height}"));
        }
        let last_row = row + height - 1;
        let last_column = column + width - 1;
        if !is_valid_row(row)
            || !is_valid_row(last_row)
            || !is_valid_column_number(column)
            || !is_valid_column_number(last_column)
        {
            return Err(format!(
                "Block at ({row}, {column}) of size {width}x{height} is out of the sheet"
            ));
        }
        let model = &self.model;
        let worksheet = model.workbook.worksheet(sheet)?;

        // The style index of every cell of the block and of the ring of
        // cells around it: a cell's extended style is a function of its own
        // style index, of its conditional formats and of the borders of its
        // four neighbours, which are functions of their style indices.
        let grid_width = (width + 2) as usize;
        let mut indices: Vec<i32> = Vec::with_capacity(grid_width * (height + 2) as usize);
        for r in (row - 1)..=(last_row + 1) {
            for c in (column - 1)..=(last_column + 1) {
                if is_valid_row(r) && is_valid_column_number(c) {
                    indices.push(model.get_cell_style_index(sheet, r, c)?);
                } else {
                    indices.push(NO_NEIGHBOUR);
                }
            }
        }
        let index_at = |r: i32, c: i32| {
            indices[(r - row + 1) as usize * grid_width + (c - column + 1) as usize]
        };

        // Which sides of a style have a border: [top, right, bottom, left]
        let mut borders: HashMap<i32, [bool; 4]> = HashMap::new();
        let mut border_on = |index: i32, side: usize| -> Result<bool, String> {
            if index == NO_NEIGHBOUR {
                return Ok(false);
            }
            if let Some(sides) = borders.get(&index) {
                return Ok(sides[side]);
            }
            let border = model.workbook.styles.get_style(index)?.border;
            let sides = [
                border.top.is_some(),
                border.right.is_some(),
                border.bottom.is_some(),
                border.left.is_some(),
            ];
            borders.insert(index, sides);
            Ok(sides[side])
        };

        let mut styles: Vec<ExtendedStyle> = Vec::new();
        let mut shared: HashMap<[i32; 5], u32> = HashMap::new();
        let mut style_indices = Vec::with_capacity((width * height) as usize);
        let mut texts = Vec::new();
        for r in row..=last_row {
            for c in column..=last_column {
                // A conditionally formatted cell gets an entry of its own:
                // its style also depends on its value.
                let key = if model.cf_cache.contains_key(&(sheet, r, c)) {
                    None
                } else {
                    // A neighbour only matters through the border on the
                    // side that faces the cell.
                    let top = index_at(r - 1, c);
                    let right = index_at(r, c + 1);
                    let bottom = index_at(r + 1, c);
                    let left = index_at(r, c - 1);
                    Some([
                        index_at(r, c),
                        if border_on(top, 2)? {
                            top
                        } else {
                            NO_NEIGHBOUR
                        },
                        if border_on(right, 3)? {
                            right
                        } else {
                            NO_NEIGHBOUR
                        },
                        if border_on(bottom, 0)? {
                            bottom
                        } else {
                            NO_NEIGHBOUR
                        },
                        if border_on(left, 1)? {
                            left
                        } else {
                            NO_NEIGHBOUR
                        },
                    ])
                };
                let index = match key.and_then(|key| shared.get(&key)) {
                    Some(index) => *index,
                    None => {
                        styles.push(self.get_extended_cell_style(sheet, r, c)?);
                        let index = (styles.len() - 1) as u32;
                        if let Some(key) = key {
                            shared.insert(key, index);
                        }
                        index
                    }
                };
                style_indices.push(index);
            }
            for (c, cell) in worksheet
                .sheet_data
                .cells_in_row_between(r, column, last_column)
            {
                let text = model.get_formatted_cell_value(sheet, r, c)?;
                if text.is_empty() {
                    continue;
                }
                texts.push(CellText {
                    row: r,
                    column: c,
                    text,
                    cell_type: cell.get_type() as i32,
                });
            }
        }
        Ok(CellsBlock {
            row,
            column,
            width,
            height,
            styles,
            style_indices,
            texts,
        })
    }
}
