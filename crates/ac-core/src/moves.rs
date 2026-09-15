//! Typed ordinary AC moves; raw official IDs exist only at this boundary.
use crate::word::{
    DEFAULT_N_GEN, Letter, Presentation, WordError, concat, conjugate, inverse, reduce,
    validate_letter,
};
pub type MoveId = u8;
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Move {
    Invert {
        target: usize,
    },
    Multiply {
        target: usize,
        other: usize,
        invert_other: bool,
    },
    Conjugate {
        target: usize,
        conjugator: Letter,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveError(pub String);
impl std::fmt::Display for MoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for MoveError {}
impl From<WordError> for MoveError {
    fn from(e: WordError) -> Self {
        Self(e.0)
    }
}
fn check_index(i: usize) -> Result<(), MoveError> {
    if i > 1 {
        Err(MoveError(format!("bad_relator_index: {i}")))
    } else {
        Ok(())
    }
}
pub fn move_from_id(id: MoveId) -> Result<Move, MoveError> {
    rows()
        .into_iter()
        .find(|(i, _, _)| *i == id)
        .map(|(_, m, _)| m)
        .ok_or_else(|| MoveError(format!("bad_move_id: {id}")))
}
pub fn move_to_id(mv: &Move) -> Result<MoveId, MoveError> {
    rows()
        .into_iter()
        .find(|(_, m, _)| m == mv)
        .map(|(id, _, _)| id)
        .ok_or_else(|| MoveError(format!("unmapped_move: {mv:?}")))
}
pub fn inverse_move(mv: &Move) -> Result<Move, MoveError> {
    let id = move_to_id(mv)?;
    let inv = rows().into_iter().find(|(i, _, _)| *i == id).unwrap().2;
    move_from_id(inv)
}
pub fn all_move_ids() -> impl Iterator<Item = MoveId> {
    0..14
}
pub fn apply_move(p: &Presentation, mv: &Move, n_gen: i8) -> Result<Presentation, MoveError> {
    let mut rel = [
        reduce(&p.relators[0], n_gen)?,
        reduce(&p.relators[1], n_gen)?,
    ];
    match mv {
        Move::Invert { target } => {
            check_index(*target)?;
            rel[*target] = inverse(&rel[*target], n_gen)?;
        }
        Move::Multiply {
            target,
            other,
            invert_other,
        } => {
            check_index(*target)?;
            check_index(*other)?;
            if target == other {
                return Err(MoveError("multiply_same_relator".into()));
            }
            let other_word = if *invert_other {
                inverse(&rel[*other], n_gen)?
            } else {
                rel[*other].clone()
            };
            rel[*target] = concat(&rel[*target], &other_word, n_gen)?;
        }
        Move::Conjugate { target, conjugator } => {
            check_index(*target)?;
            rel[*target] = conjugate(&rel[*target], *conjugator, n_gen)?;
        }
    }
    Ok(Presentation { relators: rel })
}
fn rows() -> Vec<(MoveId, Move, MoveId)> {
    vec![
        (0, Move::Invert { target: 0 }, 0),
        (1, Move::Invert { target: 1 }, 1),
        (
            2,
            Move::Multiply {
                target: 0,
                other: 1,
                invert_other: false,
            },
            3,
        ),
        (
            3,
            Move::Multiply {
                target: 0,
                other: 1,
                invert_other: true,
            },
            2,
        ),
        (
            4,
            Move::Multiply {
                target: 1,
                other: 0,
                invert_other: false,
            },
            5,
        ),
        (
            5,
            Move::Multiply {
                target: 1,
                other: 0,
                invert_other: true,
            },
            4,
        ),
        (
            6,
            Move::Conjugate {
                target: 0,
                conjugator: 1,
            },
            7,
        ),
        (
            7,
            Move::Conjugate {
                target: 0,
                conjugator: -1,
            },
            6,
        ),
        (
            8,
            Move::Conjugate {
                target: 0,
                conjugator: 2,
            },
            9,
        ),
        (
            9,
            Move::Conjugate {
                target: 0,
                conjugator: -2,
            },
            8,
        ),
        (
            10,
            Move::Conjugate {
                target: 1,
                conjugator: 1,
            },
            11,
        ),
        (
            11,
            Move::Conjugate {
                target: 1,
                conjugator: -1,
            },
            10,
        ),
        (
            12,
            Move::Conjugate {
                target: 1,
                conjugator: 2,
            },
            13,
        ),
        (
            13,
            Move::Conjugate {
                target: 1,
                conjugator: -2,
            },
            12,
        ),
    ]
}
pub fn parse_move(v: &serde_json::Value) -> Result<Move, MoveError> {
    let kind = v["kind"]
        .as_str()
        .ok_or_else(|| MoveError("missing_kind".into()))?;
    match kind {
        "invert" => {
            let target = v["target"]
                .as_u64()
                .ok_or_else(|| MoveError("bad_target".into()))? as usize;
            check_index(target)?;
            Ok(Move::Invert { target })
        }
        "multiply" => {
            let target = v["target"]
                .as_u64()
                .ok_or_else(|| MoveError("bad_target".into()))? as usize;
            let other = v["other"]
                .as_u64()
                .ok_or_else(|| MoveError("bad_other".into()))? as usize;
            check_index(target)?;
            check_index(other)?;
            if target == other {
                return Err(MoveError("multiply_same_relator".into()));
            }
            if v.get("side").and_then(|x| x.as_str()).unwrap_or("right") != "right" {
                return Err(MoveError("unsupported_side".into()));
            }
            let invert_other = v["invert_other"]
                .as_bool()
                .ok_or_else(|| MoveError("bad_invert_other".into()))?;
            Ok(Move::Multiply {
                target,
                other,
                invert_other,
            })
        }
        "conjugate" => {
            let target = v["target"]
                .as_u64()
                .ok_or_else(|| MoveError("bad_target".into()))? as usize;
            let conjugator = v["conjugator"]
                .as_i64()
                .ok_or_else(|| MoveError("bad_conjugator".into()))?
                as i8;
            check_index(target)?;
            validate_letter(conjugator, DEFAULT_N_GEN)?;
            Ok(Move::Conjugate { target, conjugator })
        }
        other => Err(MoveError(format!("unknown_kind: {other}"))),
    }
}
