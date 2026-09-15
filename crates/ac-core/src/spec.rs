//! Frozen official SAIR ordinary-AC specification boundary.
use serde_json::Value;
pub const MOVE_SPEC_VERSION: &str = "ac-r2-v1";
pub const MOVE_SPEC_HASH: &str =
    "sha256:e13f57d82b5db1a79c1bd9d5ee1f6665ac4d96a634a3656ea20c0ada82e07643";
pub const AC_TARGET: [[i8; 1]; 2] = [[1], [2]];
pub fn validate_official_move_spec(spec: &Value) -> Result<(), String> {
    if spec["move_spec_version"] != MOVE_SPEC_VERSION {
        return Err("move_spec_version mismatch".into());
    }
    if spec["move_spec_hash"] != MOVE_SPEC_HASH {
        return Err("move_spec_hash mismatch".into());
    }
    if spec["target_relators"] != serde_json::json!([[1], [2]]) {
        return Err("target_relators mismatch".into());
    }
    let moves = spec["moves"].as_array().ok_or("moves is not an array")?;
    if moves.len() != 14 {
        return Err("expected exactly 14 ordinary moves".into());
    }
    for (id, row) in moves.iter().enumerate() {
        if row["id"] != id {
            return Err(format!("move id mismatch at {id}"));
        }
        if row["inverse"].as_u64().is_none() {
            return Err(format!("missing inverse at {id}"));
        }
    }
    Ok(())
}
