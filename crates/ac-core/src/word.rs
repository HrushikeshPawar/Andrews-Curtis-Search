//! Freely reduced words over signed generator letters.
use serde::{Deserialize, Serialize};

pub const DEFAULT_N_GEN: i8 = 2;
pub type Letter = i8;
pub type Word = Vec<Letter>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presentation {
    pub relators: [Word; 2],
}
impl Presentation {
    pub fn new(r0: impl Into<Word>, r1: impl Into<Word>) -> Self {
        Self {
            relators: [r0.into(), r1.into()],
        }
    }
    pub fn target() -> Self {
        Self::new(vec![1], vec![2])
    }
    pub fn total_relator_length(&self) -> usize {
        self.relators.iter().map(Vec::len).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordError(pub String);
impl std::fmt::Display for WordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for WordError {}

pub fn validate_letter(letter: Letter, n_gen: i8) -> Result<Letter, WordError> {
    if letter == 0 || letter.unsigned_abs() > n_gen as u8 {
        return Err(WordError(format!("invalid_letter: {letter}")));
    }
    Ok(letter)
}
pub fn reduce(letters: &[Letter], n_gen: i8) -> Result<Word, WordError> {
    let mut out = Vec::with_capacity(letters.len());
    for &raw in letters {
        let a = validate_letter(raw, n_gen)?;
        if out.last().copied() == Some(-a) {
            out.pop();
        } else {
            out.push(a);
        }
    }
    Ok(out)
}
pub fn inverse(word: &[Letter], n_gen: i8) -> Result<Word, WordError> {
    Ok(reduce(word, n_gen)?.into_iter().rev().map(|a| -a).collect())
}
pub fn concat(left: &[Letter], right: &[Letter], n_gen: i8) -> Result<Word, WordError> {
    let mut joined = reduce(left, n_gen)?;
    joined.extend(reduce(right, n_gen)?);
    reduce(&joined, n_gen)
}
pub fn conjugate(word: &[Letter], conjugator: Letter, n_gen: i8) -> Result<Word, WordError> {
    let g = validate_letter(conjugator, n_gen)?;
    let mut letters = vec![g];
    letters.extend(reduce(word, n_gen)?);
    letters.push(-g);
    reduce(&letters, n_gen)
}
