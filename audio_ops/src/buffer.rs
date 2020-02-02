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

    pub fn copy_forward(&mut self, other: &Buffer<T>) {
        if self.len == other.len {
            self.data.copy_from_slice(&other.data);
            self.cursor = other.cursor;
        } else if self.len < other.len {
            self.cursor = 0;
            let tail = other.len - other.cursor;
            let i = self.len.min(tail);
            self.data[0..i].copy_from_slice(&other.data[other.cursor..(other.cursor + i)]);
            if i < self.len {
                self.data[i..self.len].copy_from_slice(&other.data[0..(self.len - i)]);
            }
        } else {
            self.cursor = 0;
            let i = other.len - other.cursor;
            self.data[0..i].copy_from_slice(&other.data[other.cursor..other.len]);
            if i < other.len {
                self.data[i..other.len].copy_from_slice(&other.data[0..other.cursor]);
            }
        }
    }

    pub fn copy_backward(&mut self, other: &Buffer<T>) {
        if self.len == other.len {
            self.data.copy_from_slice(&other.data);
            self.cursor = other.cursor;
        } else if self.len < other.len {
            self.cursor = 0;
            let head = other.cursor;
            let i = self.len.min(head);
            if i > 0 {
                self.data[(self.len - i)..self.len]
                    .copy_from_slice(&other.data[(other.cursor - i)..other.cursor]);
            }
            if i < self.len {
                self.data[0..(self.len - i)]
                    .copy_from_slice(&other.data[(other.len - self.len + i)..other.len]);
            }
        } else {
            self.cursor = 0;
            let i = other.cursor;
            if i > 0 {
                self.data[(other.len - i)..other.len].copy_from_slice(&other.data[0..i]);
            }
            self.data[0..(other.len - i)].copy_from_slice(&other.data[i..other.len]);
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
