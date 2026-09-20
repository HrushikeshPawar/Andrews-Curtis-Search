use ac_core::Presentation;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::load_submission;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoredCertificate {
    pub challenge_id: String,
    pub start: Presentation,
    pub certificate: Vec<u8>,
    pub old_length: usize,
    pub source_path: String,
}

#[derive(Debug, Default, Clone)]
pub struct StartCatalog {
    starts: HashMap<String, Presentation>,
}

impl StartCatalog {
    pub fn load(path: &Path) -> Result<Self, String> {
        let value: Value = serde_json::from_str(
            &fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
        )
        .map_err(|e| format!("{}: {e}", path.display()))?;
        let entries = value
            .get("challenges")
            .and_then(Value::as_array)
            .or_else(|| value.get("instances").and_then(Value::as_array))
            .ok_or_else(|| format!("{}: missing challenges or instances", path.display()))?;
        let mut starts = HashMap::with_capacity(entries.len());
        for entry in entries {
            let id = entry
                .get("challenge_id")
                .or_else(|| entry.get("training_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{}: start missing id", path.display()))?;
            let relators = entry
                .get("initial_relators")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("{}: {id}: missing initial_relators", path.display()))?;
            if relators.len() != 2 {
                return Err(format!("{}: {id}: expected two relators", path.display()));
            }
            let parse_word = |word: &Value| -> Result<Vec<i8>, String> {
                word.as_array()
                    .ok_or_else(|| format!("{}: {id}: relator is not an array", path.display()))?
                    .iter()
                    .map(|letter| {
                        letter
                            .as_i64()
                            .ok_or_else(|| format!("{}: {id}: non-integer letter", path.display()))
                            .map(|letter| letter as i8)
                    })
                    .collect()
            };
            starts.insert(
                id.to_string(),
                Presentation {
                    relators: [parse_word(&relators[0])?, parse_word(&relators[1])?],
                },
            );
        }
        Ok(Self { starts })
    }

    pub fn get(&self, challenge_id: &str) -> Option<&Presentation> {
        self.starts.get(challenge_id)
    }

    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }
}

pub fn load_result_jsonl(
    path: &Path,
    catalog: &StartCatalog,
) -> Result<Vec<ScoredCertificate>, String> {
    let mut best = HashMap::<String, ScoredCertificate>::new();
    for (line_number, line) in fs::read_to_string(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .lines()
        .enumerate()
    {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|e| format!("{}:{}: {e}", path.display(), line_number + 1))?;
        let id = value
            .get("challenge_id")
            .or_else(|| value.get("training_id"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!(
                    "{}:{}: missing challenge id",
                    path.display(),
                    line_number + 1
                )
            })?;
        let certificate = value
            .get("certificate")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                format!(
                    "{}:{}: missing certificate",
                    path.display(),
                    line_number + 1
                )
            })?
            .iter()
            .map(|move_id| {
                move_id
                    .as_u64()
                    .ok_or_else(|| {
                        format!("{}:{}: invalid move id", path.display(), line_number + 1)
                    })
                    .map(|move_id| move_id as u8)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let start = catalog
            .get(id)
            .ok_or_else(|| {
                format!(
                    "{}:{}: start not found for {id}",
                    path.display(),
                    line_number + 1
                )
            })?
            .clone();
        let candidate = ScoredCertificate {
            challenge_id: id.to_string(),
            start,
            old_length: certificate.len(),
            certificate,
            source_path: path.display().to_string(),
        };
        match best.get(id) {
            Some(current) if current.old_length <= candidate.old_length => {}
            _ => {
                best.insert(id.to_string(), candidate);
            }
        }
    }
    let mut records: Vec<_> = best.into_values().collect();
    records.sort_by(|a, b| a.challenge_id.cmp(&b.challenge_id));
    Ok(records)
}

pub fn load_submission_with_starts(
    path: &Path,
    catalog: &StartCatalog,
) -> Result<Vec<ScoredCertificate>, String> {
    let mut records = Vec::new();
    for (id, certificate) in load_submission(path)? {
        let start = catalog
            .get(&id)
            .ok_or_else(|| format!("{}: start not found for {id}", path.display()))?
            .clone();
        records.push(ScoredCertificate {
            challenge_id: id,
            old_length: certificate.len(),
            certificate,
            start,
            source_path: path.display().to_string(),
        });
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str, contents: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ac-q1-{name}-{nonce}.json"));
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn loads_manifest_starts_and_keeps_shortest_jsonl_certificate() {
        let starts = temp_file(
            "starts",
            r#"{"challenges":[{"challenge_id":"ac-1","initial_relators":[[1,2],[2]]}]}"#,
        );
        let results = temp_file(
            "results",
            "{\"training_id\":\"ac-1\",\"certificate\":[0,0,3]}\n{\"training_id\":\"ac-1\",\"certificate\":[3]}\n",
        );
        let catalog = StartCatalog::load(&starts).unwrap();
        let records = load_result_jsonl(&results, &catalog).unwrap();
        assert_eq!(catalog.len(), 1);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].certificate, vec![3]);
        assert_eq!(records[0].start.relators[0], vec![1, 2]);
        let _ = fs::remove_file(starts);
        let _ = fs::remove_file(results);
    }
}
