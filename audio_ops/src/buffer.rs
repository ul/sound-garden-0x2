/// Fixed-length ring buffer. Index 0 is the oldest element for `push_back`
/// users and the newest one for `push_front` users.
pub struct Buffer<T> {
    data: Vec<T>,
    cursor: usize,
    len: usize,
}

impl<T: Copy> Buffer<T> {
    pub fn new(z: T, len: usize) -> Self {
        assert!(len > 0, "Buffer length must be positive");
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
        self.cursor += 1;
        if self.cursor == self.len {
            self.cursor = 0;
        }
    }

    #[inline]
    pub fn push_front(&mut self, x: T) {
        self.cursor = if self.cursor == 0 {
            self.len - 1
        } else {
            self.cursor - 1
        };
        unsafe {
            *self.data.get_unchecked_mut(self.cursor) = x;
        }
    }

    /// Contents in index order as two contiguous slices, for tight loops that
    /// would otherwise pay for a wrap on every element.
    #[inline]
    pub fn as_slices(&self) -> (&[T], &[T]) {
        let (newer, older) = self.data.split_at(self.cursor);
        (older, newer)
    }

    #[inline]
    pub fn iter(&self) -> std::iter::Chain<std::slice::Iter<'_, T>, std::slice::Iter<'_, T>> {
        let (first, second) = self.as_slices();
        first.iter().chain(second)
    }
}

impl<T> Buffer<T> {
    /// Physical position of logical index `i`. `cursor < len`, so for the common
    /// `i < len` case one conditional subtraction replaces an integer division.
    #[inline]
    fn position(&self, i: usize) -> usize {
        let j = self.cursor + i;
        if j < self.len {
            j
        } else if j - self.len < self.len {
            j - self.len
        } else {
            j % self.len
        }
    }
}

impl<T> std::ops::Index<usize> for Buffer<T> {
    type Output = T;

    #[inline]
    fn index(&self, i: usize) -> &Self::Output {
        unsafe { self.data.get_unchecked(self.position(i)) }
    }
}

impl<T> std::ops::IndexMut<usize> for Buffer<T> {
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut Self::Output {
        let position = self.position(i);
        unsafe { self.data.get_unchecked_mut(position) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_iter_and_slices_agree_after_wrapping() {
        let mut buffer = Buffer::new(0, 5);
        for x in 1..=13 {
            buffer.push_back(x);
        }
        let expected = [9, 10, 11, 12, 13];
        let indexed = (0..5).map(|i| buffer[i]).collect::<Vec<_>>();
        let iterated = buffer.iter().copied().collect::<Vec<_>>();
        let (first, second) = buffer.as_slices();
        let sliced = [first, second].concat();
        assert_eq!(indexed, expected);
        assert_eq!(iterated, expected);
        assert_eq!(sliced, expected);
        // Out-of-range logical indices keep wrapping like `%` did.
        assert_eq!(buffer[5], 9);
        assert_eq!(buffer[12], 11);
    }

    #[test]
    fn push_front_makes_newest_index_zero() {
        let mut buffer = Buffer::new(0, 3);
        for x in 1..=4 {
            buffer.push_front(x);
        }
        assert_eq!((0..3).map(|i| buffer[i]).collect::<Vec<_>>(), [4, 3, 2]);
    }
}
