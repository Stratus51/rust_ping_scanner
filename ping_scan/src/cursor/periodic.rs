pub struct PeriodicCursor {
    index: u32,
    period: u32,
    nb_period: u32,
    offset: u32,
}

impl PeriodicCursor {
    pub fn new(index: u32, period: u32, nb_period: u32, offset: u32) -> Self {
        Self {
            index,
            period,
            nb_period,
            offset,
        }
    }
}

impl super::Cursor for PeriodicCursor {
    type Item = u32;
    fn move_next(&mut self) -> Result<(), ()> {
        if self.index >= self.nb_period - 1 {
            if self.offset >= self.period - 1 {
                return Err(());
            }
            self.offset += 1;
            self.index = 0;
        } else {
            self.index += 1;
        }
        Ok(())
    }

    fn move_prev(&mut self) -> Result<(), ()> {
        if self.index == 0 {
            if self.offset == 0 {
                return Err(());
            }
            self.offset -= 1;
            self.index = self.period - 1;
        } else {
            self.index -= 1;
        }
        Ok(())
    }

    fn value(&mut self) -> Self::Item {
        self.offset + self.index * self.period
    }
}
