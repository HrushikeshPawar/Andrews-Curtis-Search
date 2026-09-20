use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriorityBucket {
    P0,
    P1,
    P2,
    P3,
    P4,
    Unknown,
}

impl PriorityBucket {
    fn rank(self) -> u8 {
        match self {
            Self::P0 => 0,
            Self::P1 => 1,
            Self::P2 => 2,
            Self::P3 => 3,
            Self::P4 => 4,
            Self::Unknown => 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorityRecord {
    pub challenge_id: String,
    pub our_length: usize,
    pub public_best: Option<usize>,
    pub gap: Option<i64>,
    pub bucket: PriorityBucket,
}

impl PriorityRecord {
    pub fn new(
        challenge_id: impl Into<String>,
        our_length: usize,
        public_best: Option<usize>,
    ) -> Self {
        let gap = public_best.map(|best| our_length as i64 - best as i64);
        let bucket = match gap {
            Some(0) => PriorityBucket::P0,
            Some(1..=5) => PriorityBucket::P1,
            Some(6..=20) => PriorityBucket::P2,
            Some(21..=100) => PriorityBucket::P3,
            Some(_) => PriorityBucket::P4,
            None => PriorityBucket::Unknown,
        };
        Self {
            challenge_id: challenge_id.into(),
            our_length,
            public_best,
            gap,
            bucket,
        }
    }
}

pub fn rank_priorities(
    lengths: impl IntoIterator<Item = (String, usize)>,
    public_best: &HashMap<String, usize>,
) -> Vec<PriorityRecord> {
    let mut records: Vec<_> = lengths
        .into_iter()
        .map(|(id, length)| PriorityRecord::new(id.clone(), length, public_best.get(&id).copied()))
        .collect();
    records.sort_by(|a, b| {
        a.bucket
            .rank()
            .cmp(&b.bucket.rank())
            .then_with(|| a.gap.unwrap_or(i64::MAX).cmp(&b.gap.unwrap_or(i64::MAX)))
            .then_with(|| a.our_length.cmp(&b.our_length))
            .then_with(|| a.challenge_id.cmp(&b.challenge_id))
    });
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_buckets_match_measured_gaps() {
        assert_eq!(
            PriorityRecord::new("p0", 15, Some(15)).bucket,
            PriorityBucket::P0
        );
        assert_eq!(
            PriorityRecord::new("p1", 16, Some(15)).bucket,
            PriorityBucket::P1
        );
        assert_eq!(
            PriorityRecord::new("p2", 21, Some(15)).bucket,
            PriorityBucket::P2
        );
        assert_eq!(
            PriorityRecord::new("p3", 36, Some(15)).bucket,
            PriorityBucket::P3
        );
        assert_eq!(
            PriorityRecord::new("p4", 116, Some(15)).bucket,
            PriorityBucket::P4
        );
        assert_eq!(
            PriorityRecord::new("unknown", 1, None).bucket,
            PriorityBucket::Unknown
        );
    }

    #[test]
    fn ranking_is_deterministic_and_places_scoring_frontier_first() {
        let lengths = vec![
            ("unknown".to_string(), 10),
            ("near".to_string(), 16),
            ("tie".to_string(), 15),
        ];
        let public = HashMap::from([("near".to_string(), 15), ("tie".to_string(), 15)]);
        let ranked = rank_priorities(lengths, &public);
        assert_eq!(ranked[0].challenge_id, "tie");
        assert_eq!(ranked[1].challenge_id, "near");
        assert_eq!(ranked[2].bucket, PriorityBucket::Unknown);
    }
}
