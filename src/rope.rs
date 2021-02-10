use crate::text::{TextEdit, TextPosition};
use bytes::Bytes;
use ropey::{iter::Chunks, Rope};
use std::{borrow::Cow, convert::TryFrom};

trait ChunkExt<'a> {
    fn next_str(&mut self) -> &'a str;
    fn prev_str(&mut self) -> &'a str;
}

impl<'a> ChunkExt<'a> for Chunks<'a> {
    fn next_str(&mut self) -> &'a str {
        self.next().unwrap_or("")
    }

    fn prev_str(&mut self) -> &'a str {
        self.prev().unwrap_or("")
    }
}

pub struct ChunkWalker {
    rope: Rope,
    cursor: usize,
    cursor_chunk: &'static str,
    chunks: Chunks<'static>,
}

impl ChunkWalker {
    fn prev_chunk(&mut self) {
        self.cursor -= self.cursor_chunk.len();
        self.cursor_chunk = self.chunks.prev_str();
        while 0 < self.cursor && self.cursor_chunk.is_empty() {
            self.cursor_chunk = self.chunks.prev_str();
        }
    }

    fn next_chunk(&mut self) {
        self.cursor += self.cursor_chunk.len();
        self.cursor_chunk = self.chunks.next_str();
        while self.cursor < self.rope.len_bytes() && self.cursor_chunk.is_empty() {
            self.cursor_chunk = self.chunks.next_str();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn callback_adapter(mut self) -> impl FnMut(u32, tree_sitter::Point) -> Bytes {
        move |start_index, _position| {
            let start_index = start_index as usize;

            while start_index < self.cursor && 0 < self.cursor {
                self.prev_chunk();
            }

            while start_index >= self.cursor + self.cursor_chunk.len() && start_index < self.rope.len_bytes() {
                self.next_chunk();
            }

            let bytes = self.cursor_chunk.as_bytes();
            let bytes = &bytes[start_index - self.cursor ..];
            Bytes::from_static(bytes)
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn callback_adapter(mut self) -> impl FnMut(u32, Option<tree_sitter::Point>, Option<u32>) -> Bytes {
        move |start_index, _position, end_index| {
            let start_index = start_index as usize;

            while start_index < self.cursor && 0 < self.cursor {
                self.prev_chunk();
            }

            while start_index >= self.cursor + self.cursor_chunk.len() && start_index < self.rope.len_bytes() {
                self.next_chunk();
            }

            let bytes = self.cursor_chunk.as_bytes();
            let end_index = end_index.map(|i| i as usize).unwrap_or_else(|| bytes.len());
            let bytes = &bytes[start_index - self.cursor .. end_index];
            Bytes::from_static(bytes)
        }
    }
}

pub trait RopeExt {
    fn apply_edit(&mut self, edit: &TextEdit);
    fn build_edit<'a>(&self, change: &'a lsp::TextDocumentContentChangeEvent) -> anyhow::Result<TextEdit<'a>>;
    fn byte_to_lsp_position(&self, offset: usize) -> lsp::Position;
    fn byte_to_tree_sitter_point(&self, offset: usize) -> anyhow::Result<tree_sitter::Point>;
    fn chunk_walker(self, byte_idx: usize) -> ChunkWalker;
    fn lsp_position_to_core(&self, position: lsp::Position) -> anyhow::Result<TextPosition>;
    fn lsp_position_to_utf16_cu(&self, position: lsp::Position) -> anyhow::Result<u32>;
    fn lsp_range_to_tree_sitter_range(&self, range: lsp::Range) -> anyhow::Result<tree_sitter::Range>;
    fn tree_sitter_range_to_lsp_range(&self, range: tree_sitter::Range) -> lsp::Range;
    fn utf8_text_for_tree_sitter_node<'rope, 'tree>(&'rope self, node: &tree_sitter::Node<'tree>) -> Cow<'rope, str>;
}

impl RopeExt for Rope {
    fn apply_edit(&mut self, edit: &TextEdit) {
        self.remove(edit.start_char_idx .. edit.end_char_idx);
        if !edit.text.is_empty() {
            self.insert(edit.start_char_idx, &edit.text);
        }
    }

    fn build_edit<'a>(&self, change: &'a lsp::TextDocumentContentChangeEvent) -> anyhow::Result<TextEdit<'a>> {
        let text = change.text.as_str();
        let text_bytes = text.as_bytes();
        let text_end_byte_idx = text_bytes.len();

        let range = if let Some(range) = change.range {
            range
        } else {
            let start = self.byte_to_lsp_position(0);
            let end = self.byte_to_lsp_position(text_end_byte_idx);
            lsp::Range { start, end }
        };

        let start = self.lsp_position_to_core(range.start)?;
        let old_end = self.lsp_position_to_core(range.end)?;

        let new_end_byte = start.byte as usize + text_end_byte_idx;
        let new_end_position = self.byte_to_tree_sitter_point(new_end_byte)?;

        let input_edit = {
            let start_byte = start.byte;
            let old_end_byte = old_end.byte;
            let new_end_byte = u32::try_from(new_end_byte)?;
            let start_position = start.point;
            let old_end_position = old_end.point;
            tree_sitter::InputEdit::new(
                start_byte,
                old_end_byte,
                new_end_byte,
                &start_position,
                &old_end_position,
                &new_end_position,
            )
        };

        Ok(TextEdit {
            input_edit,
            start_char_idx: start.char as usize,
            end_char_idx: old_end.char as usize,
            text,
        })
    }

    fn byte_to_lsp_position(&self, byte_idx: usize) -> lsp::Position {
        let line_idx = self.byte_to_line(byte_idx);

        let line_utf16_cu_idx = {
            let char_idx = self.line_to_char(line_idx);
            self.char_to_utf16_cu(char_idx)
        };

        let character_utf16_cu_idx = {
            let char_idx = self.byte_to_char(byte_idx);
            self.char_to_utf16_cu(char_idx)
        };

        let line = line_idx;
        let character = character_utf16_cu_idx - line_utf16_cu_idx;

        lsp::Position::new(line as u32, character as u32)
    }

    fn byte_to_tree_sitter_point(&self, byte_idx: usize) -> anyhow::Result<tree_sitter::Point> {
        let line_idx = self.byte_to_line(byte_idx);
        let line_byte_idx = self.line_to_byte(line_idx);
        let row = u32::try_from(line_idx).unwrap();
        let column = u32::try_from(byte_idx - line_byte_idx)?;
        Ok(tree_sitter::Point::new(row, column))
    }

    #[allow(unsafe_code)]
    fn chunk_walker(self, byte_idx: usize) -> ChunkWalker {
        let this: &'static Rope = unsafe { std::mem::transmute::<_, _>(&self) };
        let (mut chunks, chunk_byte_idx, ..) = this.chunks_at_byte(byte_idx);
        let cursor = chunk_byte_idx;
        let cursor_chunk = chunks.next_str();
        ChunkWalker {
            rope: self,
            cursor,
            cursor_chunk,
            chunks,
        }
    }

    fn lsp_position_to_core(&self, position: lsp::Position) -> anyhow::Result<TextPosition> {
        let row_idx = position.line as usize;

        let col_code_idx = position.character as usize;

        let row_char_idx = self.line_to_char(row_idx);
        let col_char_idx = self.utf16_cu_to_char(col_code_idx);

        let row_byte_idx = self.line_to_byte(row_idx);
        let col_byte_idx = self.char_to_byte(col_char_idx);

        let row_code_idx = self.char_to_utf16_cu(row_char_idx);

        let point = {
            let row = position.line;
            let col = u32::try_from(col_byte_idx)?;
            tree_sitter::Point::new(row, col)
        };

        Ok(TextPosition {
            char: u32::try_from(row_char_idx + col_char_idx)?,
            byte: u32::try_from(row_byte_idx + col_byte_idx)?,
            code: u32::try_from(row_code_idx + col_code_idx)?,
            point,
        })
    }

    fn lsp_position_to_utf16_cu(&self, position: lsp::Position) -> anyhow::Result<u32> {
        let line_idx = position.line as usize;
        let line_utf16_cu_idx = {
            let char_idx = self.line_to_char(line_idx);
            self.char_to_utf16_cu(char_idx)
        };
        let char_utf16_cu_idx = position.character as usize;
        let result = u32::try_from(line_utf16_cu_idx + char_utf16_cu_idx)?;
        Ok(result)
    }

    fn lsp_range_to_tree_sitter_range(&self, range: lsp::Range) -> anyhow::Result<tree_sitter::Range> {
        let start = self.lsp_position_to_core(range.start)?;
        let end = self.lsp_position_to_core(range.end)?;
        let range = tree_sitter::Range::new(start.byte, end.byte, &start.point, &end.point);
        Ok(range)
    }

    fn tree_sitter_range_to_lsp_range(&self, range: tree_sitter::Range) -> lsp::Range {
        let start = self.byte_to_lsp_position(range.start_byte() as usize);
        let end = self.byte_to_lsp_position(range.end_byte() as usize);
        lsp::Range::new(start, end)
    }

    fn utf8_text_for_tree_sitter_node<'rope, 'tree>(&'rope self, node: &tree_sitter::Node<'tree>) -> Cow<'rope, str> {
        let start = self.byte_to_char(node.start_byte() as usize);
        let end = self.byte_to_char(node.end_byte() as usize);
        let slice = self.slice(start .. end);
        slice.into()
    }
}