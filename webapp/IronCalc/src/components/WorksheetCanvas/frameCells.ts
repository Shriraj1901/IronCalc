// The cells a frame paints, fetched from the engine in one call per pane
// instead of one call per cell and per lookup.
//
// The renderer asks for the style of every visible cell several times (once
// for its text, once for its fill, and once per edge of it and of its
// neighbours), and for the text of every cell. Each of those is a wasm
// crossing that serialises a style object. With thousands of visible cells
// (a sheet of tiny cells) that is most of the frame. A FrameCells fetches
// the visible rectangles as blocks (every style listed once, the non-empty
// texts, and an index per cell), and answers from them; a cell outside the
// blocks (a merge anchor or a spilling cell off screen) still goes to the
// engine.
//
// A FrameCells is valid for one render: it is built when the render starts
// and dropped when it ends.

import type {
  CellsBlock,
  CellText,
  Color,
  ExtendedCellStyle,
  Model,
} from "@ironcalc/wasm";

export interface BlockArea {
  row: number;
  column: number;
  width: number;
  height: number;
}

interface Block extends CellsBlock {
  // The text entry of each cell, by its offset in the block
  textAt: (CellText | undefined)[];
}

export class FrameCells {
  private model: Model;
  readonly sheet: number;
  // Whether the sheet shows its grid lines
  readonly showGridLines: boolean;
  private blocks: Block[];
  private colors: Map<string, string>;

  // Fetches `areas` of `sheet`. An area is clipped to the sheet; an empty
  // one is skipped. Areas may overlap; the first one holding a cell answers.
  constructor(model: Model, sheet: number, areas: BlockArea[]) {
    this.model = model;
    this.sheet = sheet;
    this.showGridLines = model.getShowGridLines(sheet);
    this.colors = new Map();
    this.blocks = [];
    for (const area of areas) {
      if (area.width <= 0 || area.height <= 0) {
        continue;
      }
      const block = model.getCellsBlock(
        sheet,
        area.row,
        area.column,
        area.width,
        area.height,
      );
      const textAt: (CellText | undefined)[] = new Array(
        block.width * block.height,
      );
      for (const text of block.texts) {
        textAt[
          (text.row - block.row) * block.width + text.column - block.column
        ] = text;
      }
      this.blocks.push({ ...block, textAt });
    }
  }

  // The offset of (row, column) in the first block holding it, if any
  private find(row: number, column: number): [Block, number] | null {
    for (const block of this.blocks) {
      if (
        row >= block.row &&
        row < block.row + block.height &&
        column >= block.column &&
        column < block.column + block.width
      ) {
        return [block, (row - block.row) * block.width + column - block.column];
      }
    }
    return null;
  }

  getCellStyle(row: number, column: number): ExtendedCellStyle {
    const found = this.find(row, column);
    if (found) {
      const [block, offset] = found;
      return block.styles[block.style_indices[offset]];
    }
    return this.model.getCellStyle(this.sheet, row, column);
  }

  getFormattedCellValue(row: number, column: number): string {
    const found = this.find(row, column);
    if (found) {
      const [block, offset] = found;
      return block.textAt[offset]?.text ?? "";
    }
    return this.model.getFormattedCellValue(this.sheet, row, column);
  }

  getCellType(row: number, column: number): number {
    const found = this.find(row, column);
    if (found) {
      const [block, offset] = found;
      // An empty cell has the type of an empty value, a number (as
      // Model.getCellType says)
      return block.textAt[offset]?.cell_type ?? 1;
    }
    return this.model.getCellType(this.sheet, row, column);
  }

  // Resolved once per distinct colour per frame
  resolveColor(color: Color): string {
    const key =
      typeof color === "string"
        ? color
        : color === undefined
          ? ""
          : `theme:${color[0]}:${color[1]}`;
    let resolved = this.colors.get(key);
    if (resolved === undefined) {
      resolved = this.model.resolveColor(color);
      this.colors.set(key, resolved);
    }
    return resolved;
  }
}
