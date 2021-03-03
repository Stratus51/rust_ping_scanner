pub struct U32Cursor {
    index: u32,
}

impl U32Cursor {
    pub fn new() -> Self {
        Self { index: 0 }
    }
}

impl super::Cursor for U32Cursor {
    type Item = u32;
    fn value(&mut self) -> Self::Item {
        self.index
    }
    fn move_next(&mut self) -> Result<(), ()> {
        if self.index == 0xFF_FF_FF_FF {
            return Err(());
        }
        self.index += 1;
        Ok(())
    }
    fn move_prev(&mut self) -> Result<(), ()> {
        if self.index == 0 {
            return Err(());
        }
        self.index -= 1;
        Ok(())
    }
}
