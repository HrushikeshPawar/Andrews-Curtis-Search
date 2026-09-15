//! Loaders for the official SAIR ordinary-AC training and manifest JSON.
use crate::{MOVE_SPEC_HASH, MOVE_SPEC_VERSION, Presentation};
use serde::Deserialize;
use std::path::Path;
#[derive(Debug, Clone, Deserialize)]
pub struct Challenge {
    pub challenge_id: String,
    pub generators: Vec<String>,
    pub initial_relators: [Vec<i8>; 2],
    pub target_relators: [Vec<i8>; 2],
    pub move_spec_version: String,
    #[serde(default)]
    pub scored: bool,
    #[serde(default)]
    pub instance_hash: Option<String>,
}
impl Challenge {
    pub fn presentation(&self) -> Presentation {
        Presentation {
            relators: self.initial_relators.clone(),
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
pub struct TrainingInstance {
    pub training_id: String,
    pub generators: Vec<String>,
    pub initial_relators: [Vec<i8>; 2],
    pub target_relators: [Vec<i8>; 2],
    pub move_spec_version: String,
    pub moves: Vec<u8>,
    pub length: usize,
}
impl TrainingInstance {
    pub fn presentation(&self) -> Presentation {
        Presentation {
            relators: self.initial_relators.clone(),
        }
    }
}
#[derive(Debug, Deserialize)]
struct TrainingFile {
    move_spec_version: String,
    move_spec_hash: String,
    instances: Vec<TrainingInstance>,
}
#[derive(Debug, Deserialize)]
struct ManifestFile {
    challenges: Vec<serde_json::Value>,
}
pub fn load_training(path: impl AsRef<Path>) -> Result<Vec<TrainingInstance>, String> {
    let file: TrainingFile =
        serde_json::from_str(&std::fs::read_to_string(path.as_ref()).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if file.move_spec_version != MOVE_SPEC_VERSION || file.move_spec_hash != MOVE_SPEC_HASH {
        return Err("training file is not pinned to the official ordinary-AC spec".into());
    }
    Ok(file.instances)
}
pub fn load_manifest(path: impl AsRef<Path>) -> Result<Vec<Challenge>, String> {
    let file = serde_json::from_str::<ManifestFile>(
        &std::fs::read_to_string(path.as_ref()).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    file.challenges
        .into_iter()
        .filter(|value| value["move_spec_version"] == MOVE_SPEC_VERSION)
        .map(|value| serde_json::from_value(value).map_err(|e| e.to_string()))
        .collect()
}
