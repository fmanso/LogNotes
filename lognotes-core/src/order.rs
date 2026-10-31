//! Fractional sibling ordering keys.
//!
//! A key is a lowercase base-36 string that sorts lexicographically in sibling order. Inserting between
//! two neighbours generates a key between them instead of renumbering the siblings, so a move rewrites
//! one row rather than a page.

use crate::error::{Error, Result};

const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const BASE: u32 = 36;

/// Longest key we will generate. Reaching it means one gap has been split 64 times, which is the point
/// where a person should be told rather than a key silently growing forever.
const MAX_LEN: usize = 64;

fn digit_of(byte: u8) -> Option<u32> {
    ALPHABET.iter().position(|d| *d == byte).map(|p| p as u32)
}

fn char_of(digit: u32) -> char {
    ALPHABET[digit as usize] as char
}

/// A key strictly between `prev` and `next`, either of which may be open-ended.
///
/// `prev` must sort before `next` when both are given.
pub fn key_between(prev: Option<&str>, next: Option<&str>) -> Result<String> {
    if let (Some(p), Some(n)) = (prev, next)
        && p >= n
    {
        return Err(Error::InvalidPosition {
            prev: prev.map(str::to_owned),
            next: next.map(str::to_owned),
        });
    }

    let prev_str = prev.unwrap_or_default();
    let prev = prev_str.as_bytes();
    let next = next.map(str::as_bytes);
    let mut out = String::new();

    for i in 0..MAX_LEN {
        let a = prev.get(i).and_then(|b| digit_of(*b)).unwrap_or(0);
        let b = match next {
            Some(next) => next.get(i).and_then(|b| digit_of(*b)).unwrap_or(BASE),
            None => BASE,
        };

        if b > a + 1 {
            out.push(char_of((a + b) / 2));
            return Ok(out);
        }

        // `b` is at most one digit above `a`: copy `a` and decide on the next digit.
        out.push(char_of(a));
    }

    Err(Error::InvalidPosition {
        prev: Some(prev_str.to_string()),
        next: next.map(|n| String::from_utf8_lossy(n).into_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_between(prev: Option<&str>, next: Option<&str>) {
        let key = key_between(prev, next).expect("a key should fit");
        if let Some(p) = prev {
            assert!(p < key.as_str(), "{p} < {key}");
        }
        if let Some(n) = next {
            assert!(key.as_str() < n, "{key} < {n}");
        }
    }

    #[test]
    fn first_key_of_an_empty_page() {
        let key = key_between(None, None).unwrap();
        assert_eq!(key.len(), 1);
    }

    #[test]
    fn stays_between_neighbours() {
        assert_between(None, None);
        assert_between(None, Some("i"));
        assert_between(Some("i"), None);
        assert_between(Some("0"), Some("1"));
        assert_between(Some("a"), Some("b"));
        assert_between(Some("ab"), Some("ac"));
        assert_between(Some("z"), None);
    }

    #[test]
    fn appended_keys_keep_increasing() {
        let mut prev: Option<String> = None;
        let mut seen = Vec::new();
        for _ in 0..200 {
            let key = key_between(prev.as_deref(), None).unwrap();
            assert!(prev.as_deref().map(|p| p < key.as_str()).unwrap_or(true));
            prev = Some(key.clone());
            seen.push(key);
        }
        assert!(seen.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn repeated_splits_stay_ordered() {
        let mut lo = "a".to_string();
        let hi = "b".to_string();
        for _ in 0..40 {
            let key = key_between(Some(&lo), Some(&hi)).unwrap();
            assert!(lo < key && key < hi);
            lo = key;
        }
    }

    #[test]
    fn rejects_inverted_bounds() {
        assert!(key_between(Some("b"), Some("a")).is_err());
        assert!(key_between(Some("b"), Some("b")).is_err());
    }
}
