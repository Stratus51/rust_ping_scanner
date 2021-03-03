use crate::cursor::Cursor;

pub struct U32SamplingCursor {
    pub depth: u32,
    pub index: u32,
    pub max_depth: u32,

    offset: u32,
    period: u32,
    max: u32,
}
pub const MAX_DEPTH: u32 = 31;

fn period_from_depth(max_depth: u32, depth: u32) -> u32 {
    1 << (max_depth - depth)
}

fn max_from_depth(depth: u32) -> u32 {
    1 << depth
}

// MD: 3
//        0 1 2 3 4 5 6 7
//
// 0: 8           4
// 1: 4       2       6
// 2: 2     1   3   5   7

impl U32SamplingCursor {
    pub fn new(depth: u32, index: u32, max_depth: u32) -> Self {
        if max_depth > MAX_DEPTH {
            panic!("max_depth ({}) cannot be > {}", max_depth, MAX_DEPTH);
        }
        let mut ret = Self {
            depth,
            index,
            max_depth,

            offset: 0,
            period: 0,
            max: 0,
        };
        ret.refresh_meta();
        ret
    }

    fn refresh_meta(&mut self) {
        self.period = period_from_depth(self.max_depth, self.depth);
        self.offset = self.period / 2;
        self.max = max_from_depth(self.depth);
    }
}

impl Cursor for U32SamplingCursor {
    type Item = u32;
    fn move_next(&mut self) -> Result<(), ()> {
        if self.index >= self.max - 1 {
            if self.depth >= self.max_depth - 1 {
                return Err(());
            }
            self.depth += 1;
            self.refresh_meta();
            self.index = 0;
        } else {
            self.index += 1;
        }
        Ok(())
    }

    fn move_prev(&mut self) -> Result<(), ()> {
        if self.index == 0 {
            if self.depth == 0 {
                return Err(());
            }
            self.depth -= 1;
            self.refresh_meta();
            self.index = self.max - 1;
        } else {
            self.index -= 1;
        }
        Ok(())
    }

    fn value(&mut self) -> u32 {
        self.offset + self.index * self.period
    }
}
