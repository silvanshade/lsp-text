#[derive(Clone, Debug, PartialEq)]
pub struct TextPosition {
    pub char: u32,
    pub byte: u32,
    pub code: u32,
    pub point: tree_sitter::Point,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit<'a> {
    pub input_edit: tree_sitter::InputEdit,
    pub start_char_idx: usize,
    pub end_char_idx: usize,
    pub text: &'a str,
}

impl<'a> TextEdit<'a> {
    pub fn range(&self) -> tree_sitter::Range {
        let start_byte = self.input_edit.start_byte();
        let end_byte = self.input_edit.new_end_byte();
        let start_point = &self.input_edit.start_position();
        let end_point = &self.input_edit.new_end_position();
        tree_sitter::Range::new(start_byte, end_byte, start_point, end_point)
    }
}
