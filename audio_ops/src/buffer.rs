pub struct Buffer<T> {
    data: Vec<T>,
    cursor: usize,
    len: usize,
}

impl<T: Copy> Buffer<T> {
    pub fn new(z: T, len: usize) -> Self {
        Buffer {
            data: vec![z; len],
            cursor: 0,
            len,
        }
    }

    pub fn steal_same_size(&mut self, other: &mut Buffer<T>) -> bool {
        if self.len == other.len {
            std::mem::swap(self, other);
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn push_back(&mut self, x: T) {
        unsafe {
            *self.data.get_unchecked_mut(self.cursor) = x;
        }
        self.cursor = (self.cursor + 1) % self.len;
    }

    #[inline]
    pub fn push_front(&mut self, x: T) {
        self.cursor = (self.len + self.cursor - 1) % self.len;
        unsafe {
            *self.data.get_unchecked_mut(self.cursor) = x;
        }
    }

    #[inline]
    pub fn iter<'a>(&'a self) -> Iter<'a, T> {
        Iter {
            buffer: self,
            index: 0,
        }
    }
}

impl<T> std::ops::Index<usize> for Buffer<T> {
    type Output = T;

    #[inline]
    fn index(&self, i: usize) -> &Self::Output {
        unsafe { self.data.get_unchecked((self.cursor + i) % self.len) }
    }
}

impl<T> std::ops::IndexMut<usize> for Buffer<T> {
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut Self::Output {
        unsafe { self.data.get_unchecked_mut((self.cursor + i) % self.len) }
    }
}

pub struct Iter<'a, T> {
    buffer: &'a Buffer<T>,
    index: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.buffer.len {
            let x = &self.buffer[self.index];
            self.index += 1;
            Some(x)
        } else {
            None
        }
    }
}
