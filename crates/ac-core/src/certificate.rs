use crate::{DEFAULT_N_GEN, MoveId, Presentation, apply_move, move_from_id};
pub type Certificate = Vec<MoveId>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    InvalidMove(String),
    NotTarget(Presentation),
}
impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ReplayError {}
pub fn is_ac_target(p: &Presentation) -> bool {
    p.relators == [vec![1], vec![2]]
}
pub fn replay_certificate(
    start: &Presentation,
    certificate: &[MoveId],
) -> Result<Presentation, ReplayError> {
    let mut state = Presentation {
        relators: [
            crate::reduce(&start.relators[0], DEFAULT_N_GEN)
                .map_err(|e| ReplayError::InvalidMove(e.0))?,
            crate::reduce(&start.relators[1], DEFAULT_N_GEN)
                .map_err(|e| ReplayError::InvalidMove(e.0))?,
        ],
    };
    for &id in certificate {
        let mv = move_from_id(id).map_err(|e| ReplayError::InvalidMove(e.0))?;
        state =
            apply_move(&state, &mv, DEFAULT_N_GEN).map_err(|e| ReplayError::InvalidMove(e.0))?;
    }
    if is_ac_target(&state) {
        Ok(state)
    } else {
        Err(ReplayError::NotTarget(state))
    }
}
