use ac_core::{
    MOVE_SPEC_HASH, MOVE_SPEC_VERSION, Move, Presentation, apply_move,
    data::{load_manifest, load_training},
    inverse, inverse_move, move_from_id, move_to_id, reduce, replay_certificate,
    validate_official_move_spec,
};
use serde_json::Value;
use std::path::PathBuf;

fn fixture(name: &str) -> Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/v1");
    serde_json::from_str(&std::fs::read_to_string(root.join(name)).unwrap()).unwrap()
}
fn word(v: &Value) -> Vec<i8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_i64().unwrap() as i8)
        .collect()
}
fn presentation(v: &Value) -> Presentation {
    let a = v.as_array().unwrap();
    Presentation::new(word(&a[0]), word(&a[1]))
}

#[test]
fn shared_word_fixtures_and_laws() {
    let data = fixture("words.json");
    for case in data["cases"].as_array().unwrap() {
        let input = word(&case["input"]);
        let reduced = reduce(&input, 2).unwrap();
        assert_eq!(reduced, word(&case["reduced"]), "{}", case["id"]);
        assert_eq!(
            inverse(&reduced, 2).unwrap(),
            word(&case["inverse_of_reduced"])
        );
        assert_eq!(inverse(&inverse(&reduced, 2).unwrap(), 2).unwrap(), reduced);
    }
    for case in data["errors"].as_array().unwrap() {
        assert!(
            reduce(
                &word(&case["input"]),
                case.get("n_gen").and_then(Value::as_i64).unwrap_or(2) as i8
            )
            .is_err()
        );
    }
}

#[test]
fn all_official_moves_round_trip() {
    let data = fixture("moves.json");
    for case in data["cases"].as_array().unwrap() {
        let id = case["move_id"].as_u64().unwrap() as u8;
        let mv = move_from_id(id).unwrap();
        assert_eq!(move_to_id(&mv).unwrap(), id);
        let start = presentation(&case["state"]);
        let got = apply_move(&start, &mv, 2).unwrap();
        assert_eq!(got, presentation(&case["expected_state"]), "{}", case["id"]);
        assert_eq!(
            apply_move(&got, &inverse_move(&mv).unwrap(), 2).unwrap(),
            Presentation::new(
                reduce(&start.relators[0], 2).unwrap(),
                reduce(&start.relators[1], 2).unwrap()
            )
        );
    }
    assert!(move_from_id(14).is_err());
}

#[test]
fn certificate_replay_and_validation() {
    let data = fixture("certificates.json");
    for case in data["cases"].as_array().unwrap() {
        let start = presentation(&case["start"]);
        let ids: Vec<u8> = case["move_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as u8)
            .collect();
        let end = replay_certificate(&start, &ids).unwrap();
        assert_eq!(end, presentation(&case["end"]));
    }
    let invalid = replay_certificate(&Presentation::new(vec![1, 2], vec![2]), &[3]);
    assert!(invalid.is_ok());
    let invalid_target = replay_certificate(&Presentation::new(vec![1], vec![2]), &[0]);
    assert!(invalid_target.is_err());
}

#[test]
fn official_spec_and_training_are_pinned() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/sair");
    let spec: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("verifier/data/move_spec.json")).unwrap(),
    )
    .unwrap();
    validate_official_move_spec(&spec).unwrap();
    assert_eq!(spec["move_spec_version"], MOVE_SPEC_VERSION);
    assert_eq!(spec["move_spec_hash"], MOVE_SPEC_HASH);
    let instances = load_training(root.join("examples/training_424.json")).unwrap();
    assert_eq!(instances.len(), 424);
    for item in instances {
        assert!(
            replay_certificate(&item.presentation(), &item.moves).is_ok(),
            "{}",
            item.training_id
        );
    }
    assert_eq!(
        load_manifest(root.join("examples/training_manifest.json"))
            .unwrap()
            .len(),
        424
    );
}

#[test]
fn typed_move_shape_is_not_raw_integer_logic() {
    assert!(matches!(
        move_from_id(0).unwrap(),
        Move::Invert { target: 0 }
    ));
}
