pub struct U32SamplingIterator {
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

impl U32SamplingIterator {
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

    fn current(&self) -> u32 {
        self.offset + self.index * self.period
    }
}

impl Iterator for U32SamplingIterator {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        self.index += 1;
        if self.index >= self.max {
            self.depth += 1;
            if self.depth >= self.max_depth {
                return None;
            }
            self.index = 0;
            self.refresh_meta();
        }
        Some(self.current())
    }
}
