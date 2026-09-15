//! Trusted ordinary Andrews–Curtis semantics for the SAIR rank-2 problem.
pub mod certificate;
pub mod data;
pub mod moves;
pub mod spec;
pub mod word;

pub use certificate::{Certificate, ReplayError, is_ac_target, replay_certificate};
pub use moves::{
    Move, MoveError, MoveId, all_move_ids, apply_move, inverse_move, move_from_id, move_to_id,
    parse_move,
};
pub use spec::{AC_TARGET, MOVE_SPEC_HASH, MOVE_SPEC_VERSION, validate_official_move_spec};
pub use word::{
    DEFAULT_N_GEN, Letter, Presentation, Word, WordError, concat, conjugate, inverse, reduce,
    validate_letter,
};
