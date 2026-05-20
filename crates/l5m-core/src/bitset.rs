#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BitSet {
    size: usize,
    words: Vec<u64>,
}

impl BitSet {
    pub fn new(size: usize) -> Self {
        Self {
            size,
            words: vec![0; size.div_ceil(64)],
        }
    }

    pub fn full(size: usize) -> Self {
        let mut set = Self {
            size,
            words: vec![u64::MAX; size.div_ceil(64)],
        };
        set.clear_unused_bits();
        set
    }

    pub fn set(&mut self, index: usize) {
        assert!(index < self.size, "bit index out of range");
        self.words[index / 64] |= 1u64 << (index % 64);
    }

    pub fn clear(&mut self, index: usize) {
        assert!(index < self.size, "bit index out of range");
        self.words[index / 64] &= !(1u64 << (index % 64));
    }

    pub fn get(&self, index: usize) -> bool {
        assert!(index < self.size, "bit index out of range");
        (self.words[index / 64] & (1u64 << (index % 64))) != 0
    }

    pub fn and_assign(&mut self, other: &Self) {
        assert_eq!(self.size, other.size, "bitset sizes differ");
        for (left, right) in self.words.iter_mut().zip(&other.words) {
            *left &= *right;
        }
    }

    pub fn or_assign(&mut self, other: &Self) {
        assert_eq!(self.size, other.size, "bitset sizes differ");
        for (left, right) in self.words.iter_mut().zip(&other.words) {
            *left |= *right;
        }
        self.clear_unused_bits();
    }

    pub fn count_ones(&self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    pub fn iter_ones(&self) -> BitSetOnes<'_> {
        BitSetOnes {
            bitset: self,
            word_index: 0,
            current_word: self.words.first().copied().unwrap_or(0),
        }
    }

    pub fn len(&self) -> usize {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    fn clear_unused_bits(&mut self) {
        let extra = self.size % 64;
        if extra != 0 {
            if let Some(last) = self.words.last_mut() {
                *last &= (1u64 << extra) - 1;
            }
        }
    }
}

pub struct BitSetOnes<'a> {
    bitset: &'a BitSet,
    word_index: usize,
    current_word: u64,
}

impl Iterator for BitSetOnes<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.word_index < self.bitset.words.len() {
            if self.current_word != 0 {
                let bit = self.current_word.trailing_zeros() as usize;
                self.current_word &= self.current_word - 1;
                let index = self.word_index * 64 + bit;
                if index < self.bitset.size {
                    return Some(index);
                }
            } else {
                self.word_index += 1;
                self.current_word = self.bitset.words.get(self.word_index).copied().unwrap_or(0);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::BitSet;

    #[test]
    fn bitset_operations_work() {
        let mut a = BitSet::new(130);
        a.set(0);
        a.set(64);
        a.set(129);
        assert!(a.get(64));
        assert_eq!(a.count_ones(), 3);

        a.clear(64);
        assert!(!a.get(64));
        assert_eq!(a.iter_ones().collect::<Vec<_>>(), vec![0, 129]);

        let mut b = BitSet::full(130);
        b.clear(0);
        a.and_assign(&b);
        assert_eq!(a.iter_ones().collect::<Vec<_>>(), vec![129]);

        let mut c = BitSet::new(130);
        c.set(2);
        a.or_assign(&c);
        assert_eq!(a.iter_ones().collect::<Vec<_>>(), vec![2, 129]);
    }
}
