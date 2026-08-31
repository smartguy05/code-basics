use super::*;

#[test]
fn every_feature_has_a_distinct_stable_id() {
    let mut ids: Vec<&str> = FeatureId::ALL.iter().map(|f| f.id()).collect();
    ids.sort_unstable();
    let count = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), count, "two features share an id");
    assert_eq!(ids, vec!["askCodebase", "sqlConsole"]);
}

#[test]
fn an_id_round_trips_through_from_id() {
    for feature in FeatureId::ALL {
        assert_eq!(FeatureId::from_id(feature.id()), Ok(feature));
    }
}

#[test]
fn an_unknown_id_is_an_error_naming_it() {
    let err = FeatureId::from_id("telepathy").unwrap_err();
    assert!(err.contains("telepathy"), "{err}");
}

#[test]
fn an_absent_id_falls_back_to_its_built_in_default() {
    let file = FeaturesFile::default();
    assert!(file.enabled.is_empty(), "nothing is recorded");
    for feature in FeatureId::ALL {
        assert_eq!(
            file.is_enabled(feature),
            feature.default_enabled(),
            "{} should fall back, not read as off",
            feature.id()
        );
    }
}

#[test]
fn the_default_for_a_fresh_install_is_on() {
    // A launch that never saw an installer -- cargo run, a dev checkout, an
    // AppImage -- must not look broken.
    for feature in FeatureId::ALL {
        assert!(feature.default_enabled(), "{} defaults off", feature.id());
    }
}

#[test]
fn an_explicit_off_wins_over_the_default() {
    let mut file = FeaturesFile::default();
    file.set(FeatureId::SqlConsole, false);
    assert!(!file.is_enabled(FeatureId::SqlConsole));
    assert!(
        file.is_enabled(FeatureId::AskCodebase),
        "untouched by the other"
    );
}

#[test]
fn list_reports_every_known_feature_with_its_state() {
    let mut file = FeaturesFile::default();
    file.set(FeatureId::AskCodebase, false);

    let rows = file.list();
    assert_eq!(rows.len(), FeatureId::ALL.len());

    let ask = rows.iter().find(|r| r.id == "askCodebase").unwrap();
    assert!(!ask.enabled);
    assert_eq!(ask.label, "Ask the codebase");
    assert!(!ask.description.is_empty(), "a checkbox needs a caption");

    let sql = rows.iter().find(|r| r.id == "sqlConsole").unwrap();
    assert!(sql.enabled, "an unrecorded feature lists as its default");
}

#[test]
fn serialisation_shape_pins_the_wire_keys() {
    let mut file = FeaturesFile::default();
    file.set(FeatureId::SqlConsole, false);

    let value = serde_json::to_value(&file).unwrap();
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["enabled", "version"]);
    assert_eq!(value["enabled"]["sqlConsole"], serde_json::json!(false));

    let info = &file.list()[0];
    let value = serde_json::to_value(info).unwrap();
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["description", "enabled", "id", "label"]);
}
