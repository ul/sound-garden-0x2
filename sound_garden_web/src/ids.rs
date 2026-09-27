//! Stable node ids for programs edited as plain text.
//!
//! The VM carries op state across a reload by node id, which the grid editor gets for free from
//! its nodes. A text editor has only words, so ids are inherited through a word diff: words the
//! edit didn't touch keep their ids, and words replaced in place inherit the ids of the words
//! they replaced, so changing `s` to `t` keeps the oscillator's phase just like in the editor.

pub struct Ids {
    words: Vec<(String, u64)>,
    next: u64,
}

/// Above this many diff cells (old × new words) ids are inherited by position instead.
const MAX_DIFF_CELLS: usize = 4_000_000;

impl Default for Ids {
    fn default() -> Self {
        Ids {
            words: Vec::new(),
            next: 1,
        }
    }
}

impl Ids {
    /// Ids for `words`, inherited from the previous call where the words line up.
    pub fn assign(&mut self, words: &[&str]) -> Vec<u64> {
        let old = std::mem::take(&mut self.words);
        let mut ids = vec![0; words.len()];
        let mut pair_gap =
            |ids: &mut [u64], old_gap: &[(String, u64)], new_gap: std::ops::Range<usize>| {
                for (k, j) in new_gap.enumerate() {
                    ids[j] = match old_gap.get(k) {
                        Some((_, id)) => *id,
                        None => {
                            self.next += 1;
                            self.next - 1
                        }
                    };
                }
            };

        let (n, m) = (old.len(), words.len());
        if n.saturating_mul(m) > MAX_DIFF_CELLS {
            pair_gap(&mut ids, &old, 0..m);
        } else {
            // Longest common subsequence of words; unmatched runs between matches pair up in order.
            let mut lcs = vec![0u32; (n + 1) * (m + 1)];
            let at = |i: usize, j: usize| i * (m + 1) + j;
            for i in (0..n).rev() {
                for j in (0..m).rev() {
                    lcs[at(i, j)] = if old[i].0 == words[j] {
                        lcs[at(i + 1, j + 1)] + 1
                    } else {
                        lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
                    };
                }
            }
            let (mut i, mut j) = (0, 0);
            let (mut gap_i, mut gap_j) = (0, 0);
            while i < n && j < m {
                if old[i].0 == words[j] {
                    pair_gap(&mut ids, &old[gap_i..i], gap_j..j);
                    ids[j] = old[i].1;
                    i += 1;
                    j += 1;
                    (gap_i, gap_j) = (i, j);
                } else if lcs[at(i + 1, j)] >= lcs[at(i, j + 1)] {
                    i += 1;
                } else {
                    j += 1;
                }
            }
            pair_gap(&mut ids, &old[gap_i..], gap_j..m);
        }

        self.words = words
            .iter()
            .map(|w| w.to_string())
            .zip(ids.iter().copied())
            .collect();
        ids
    }

    /// Forget the previous program: the next one starts with fresh ids.
    pub fn forget(&mut self) {
        self.words.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<&str> {
        s.split_whitespace().collect()
    }

    #[test]
    fn untouched_words_keep_their_ids() {
        let mut ids = Ids::default();
        let a = ids.assign(&words("440 s 0.2 *"));
        let b = ids.assign(&words("440 s 0.2 * 0.5 *"));
        assert_eq!(a, b[..4]);
        assert!(!a.contains(&b[4]) && !a.contains(&b[5]));
    }

    #[test]
    fn a_word_replaced_in_place_inherits_its_id() {
        let mut ids = Ids::default();
        let a = ids.assign(&words("110 s 220 s +"));
        let b = ids.assign(&words("110 t 220 s +"));
        assert_eq!(a, b);
    }

    #[test]
    fn insertions_before_a_word_do_not_steal_its_id() {
        let mut ids = Ids::default();
        let a = ids.assign(&words("110 s"));
        let b = ids.assign(&words("2 s 1 + 110 * s"));
        assert_eq!((b[4], b[6]), (a[0], a[1]));
    }

    #[test]
    fn forgetting_starts_fresh() {
        let mut ids = Ids::default();
        let a = ids.assign(&words("110 s"));
        ids.forget();
        let b = ids.assign(&words("110 s"));
        assert!(a.iter().all(|id| !b.contains(id)));
    }
}
