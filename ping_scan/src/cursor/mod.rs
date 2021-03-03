pub mod periodic;
pub mod u32;
pub mod u32_sampling;

pub struct FilterCursor<C: Cursor, P> {
    cursor: C,
    predicate: P,
}

impl<C: Cursor, P> Cursor for FilterCursor<C, P>
where
    P: FnMut(&C::Item) -> bool,
{
    type Item = C::Item;
    fn value(&mut self) -> Self::Item {
        self.cursor.value()
    }
    fn move_next(&mut self) -> Result<(), ()> {
        let mut moves = 0;
        while self.cursor.move_next().is_ok() {
            moves += 1;
            let value = self.value();
            if (self.predicate)(&value) {
                return Ok(());
            }
        }
        for _ in 0..moves {
            let _ = self.move_prev();
        }
        Err(())
    }
    fn move_prev(&mut self) -> Result<(), ()> {
        let mut moves = 0;
        while self.cursor.move_prev().is_ok() {
            moves += 1;
            let value = self.value();
            if (self.predicate)(&value) {
                return Ok(());
            }
        }
        for _ in 0..moves {
            let _ = self.move_next();
        }
        Err(())
    }
}

pub struct MapCursor<C: Cursor, F> {
    cursor: C,
    f: F,
}

impl<C: Cursor, B, F> Cursor for MapCursor<C, F>
where
    F: FnMut(C::Item) -> B,
{
    type Item = B;
    fn value(&mut self) -> B {
        let value = self.cursor.value();
        (self.f)(value)
    }
    fn move_next(&mut self) -> Result<(), ()> {
        self.cursor.move_next()
    }
    fn move_prev(&mut self) -> Result<(), ()> {
        self.cursor.move_prev()
    }
}

pub struct SkipCursor<C: Cursor> {
    cursor: C,
    consumed_items: usize,
}

impl<C: Cursor> SkipCursor<C> {
    fn new(mut cursor: C, n: usize) -> Result<Self, ()> {
        for _ in 0..n {
            if cursor.move_next().is_err() {
                return Err(());
            }
        }
        Ok(Self {
            cursor,
            consumed_items: 0,
        })
    }
}

impl<C: Cursor> Cursor for SkipCursor<C> {
    type Item = C::Item;
    fn value(&mut self) -> Self::Item {
        self.cursor.value()
    }

    fn move_next(&mut self) -> Result<(), ()> {
        self.cursor.move_next()?;
        self.consumed_items += 1;
        Ok(())
    }

    fn move_prev(&mut self) -> Result<(), ()> {
        if self.consumed_items == 0 {
            return Err(());
        }
        self.cursor.move_prev()?;
        self.consumed_items -= 1;
        Ok(())
    }
}

pub struct TakeCursor<C: Cursor> {
    cursor: C,
    remaining: usize,
}

impl<C: Cursor> Cursor for TakeCursor<C> {
    type Item = C::Item;
    fn value(&mut self) -> Self::Item {
        self.cursor.value()
    }

    fn move_next(&mut self) -> Result<(), ()> {
        if self.remaining == 0 {
            return Err(());
        }
        self.cursor.move_next()?;
        self.remaining -= 1;
        Ok(())
    }

    fn move_prev(&mut self) -> Result<(), ()> {
        self.cursor.move_prev()?;
        self.remaining += 1;
        Ok(())
    }
}

pub trait Cursor {
    type Item;
    fn value(&mut self) -> Self::Item;
    fn move_next(&mut self) -> Result<(), ()>;
    fn move_prev(&mut self) -> Result<(), ()>;
}

pub struct CursorIterator<C: Cursor> {
    cursor: C,
    done: bool,
}

impl<C: Cursor> CursorIterator<C> {
    pub fn new(cursor: C) -> Self {
        Self {
            cursor,
            done: false,
        }
    }
}

impl<C: Cursor> Iterator for CursorIterator<C> {
    type Item = C::Item;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            None
        } else {
            let res = Some(self.cursor.value());
            if self.cursor.move_next().is_err() {
                self.done = true;
            }
            res
        }
    }
}

pub trait CursorExt: Cursor {
    fn to_iter(self) -> CursorIterator<Self>
    where
        Self: Sized,
    {
        CursorIterator::new(self)
    }

    fn filter<P>(self, predicate: P) -> FilterCursor<Self, P>
    where
        Self: Sized,
        P: FnMut(&Self::Item) -> bool,
    {
        FilterCursor {
            cursor: self,
            predicate,
        }
    }

    fn map<B, F>(self, f: F) -> MapCursor<Self, F>
    where
        Self: Sized,
        F: FnMut(Self::Item) -> B,
    {
        MapCursor { cursor: self, f }
    }

    fn skip(self, n: usize) -> Result<SkipCursor<Self>, ()>
    where
        Self: Sized,
    {
        SkipCursor::new(self, n)
    }

    fn take(self, n: usize) -> TakeCursor<Self>
    where
        Self: Sized,
    {
        TakeCursor {
            cursor: self,
            remaining: n,
        }
    }
}
impl<T: ?Sized> CursorExt for T where T: Cursor {}
impl<T: ?Sized> Cursor for Box<T>
where
    T: Cursor,
{
    type Item = T::Item;
    fn value(&mut self) -> Self::Item {
        self.as_mut().value()
    }
    fn move_next(&mut self) -> Result<(), ()> {
        self.as_mut().move_next()
    }
    fn move_prev(&mut self) -> Result<(), ()> {
        self.as_mut().move_prev()
    }
}
