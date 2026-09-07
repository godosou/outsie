use plist::Value;
use repose_authdb_policy::{PolicyError, PolicySpec, ScreenSaverPolicy};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/authorizationdb"
);

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIXTURES}/{name}"))
        .unwrap_or_else(|error| panic!("failed to read fixture {name}: {error}"))
}

fn parse_value(bytes: &[u8]) -> Value {
    Value::from_reader(std::io::Cursor::new(bytes)).expect("valid output plist")
}

fn rules(value: &Value) -> &[Value] {
    value
        .as_dictionary()
        .and_then(|root| root.get("rule"))
        .and_then(Value::as_array)
        .expect("installed rule array")
}

#[test]
fn installs_before_fallback_from_stock_string_rule() {
    let input = fixture("stock-string.plist");

    let installed = ScreenSaverPolicy::parse(&input)
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .expect("safe stock policy");

    assert!(installed.has_exactly_one_password_fallback());
    assert_eq!(
        installed
            .repose_candidate_index()
            .expect("Repose candidate")
            + 1,
        installed
            .password_fallback_index()
            .expect("password fallback")
    );

    let output = installed.to_xml_bytes().expect("serialize policy");
    let root = parse_value(&output);
    assert_eq!(rules(&root).len(), 2);
    assert_eq!(
        root.as_dictionary().and_then(|dict| dict.get("k-of-n")),
        Some(&Value::Integer(1.into()))
    );
}

#[test]
fn installs_from_stock_array_rule() {
    let installed = ScreenSaverPolicy::parse(&fixture("stock-array.plist"))
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .expect("safe stock array policy");

    assert_eq!(installed.repose_candidate_index(), Some(0));
    assert_eq!(installed.password_fallback_index(), Some(1));
    let output = parse_value(&installed.to_xml_bytes().expect("serialize policy"));
    assert_eq!(
        output.as_dictionary().and_then(|dict| dict.get("k-of-n")),
        Some(&Value::Integer(1.into()))
    );
}

#[test]
fn preserves_third_party_candidates_unknown_keys_and_value_types() {
    let before = parse_value(&fixture("third-party.plist"));
    let before_root = before.as_dictionary().expect("fixture dictionary");
    let before_rules = rules(&before);

    let installed = ScreenSaverPolicy::parse(&fixture("third-party.plist"))
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .expect("safe third-party policy");
    let output = parse_value(&installed.to_xml_bytes().expect("serialize policy"));
    let output_root = output.as_dictionary().expect("output dictionary");
    let output_rules = rules(&output);

    assert_eq!(&output_rules[..2], &before_rules[..2]);
    assert_eq!(&output_rules[3..], &before_rules[2..]);
    for (key, expected) in before_root {
        if key != "rule" {
            assert_eq!(
                output_root.get(key),
                Some(expected),
                "unknown or metadata key `{key}` changed"
            );
        }
    }
}

#[test]
fn repeated_install_is_byte_idempotent() {
    let once = ScreenSaverPolicy::parse(&fixture("already-installed.plist"))
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .expect("already-installed policy")
        .to_xml_bytes()
        .expect("serialize once");
    let twice = ScreenSaverPolicy::parse(&once)
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .expect("install twice")
        .to_xml_bytes()
        .expect("serialize twice");

    assert_eq!(once, twice);
}

#[test]
fn install_surgically_moves_a_single_misplaced_repose_candidate() {
    let installed = ScreenSaverPolicy::parse(&fixture("misplaced-repose.plist"))
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .expect("move the owned candidate");
    let once = installed.to_xml_bytes().expect("serialize repaired policy");
    let value = parse_value(&once);
    let names: Vec<_> = rules(&value)
        .iter()
        .map(|candidate| candidate.as_string().expect("string candidate"))
        .collect();

    assert_eq!(
        names,
        vec![
            "com.example.security-key",
            "ai.repose.unlock",
            "use-login-window-ui"
        ]
    );

    let twice = ScreenSaverPolicy::parse(&once)
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .and_then(|policy| policy.to_xml_bytes())
        .expect("second install");
    assert_eq!(once, twice);
}

#[test]
fn remove_deletes_only_repose_and_keeps_current_third_party_state() {
    let installed = ScreenSaverPolicy::parse(&fixture("third-party.plist"))
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .expect("install Repose");
    let installed_value = parse_value(&installed.to_xml_bytes().expect("serialize installed"));
    let expected_third_party = vec![
        rules(&installed_value)[0].clone(),
        rules(&installed_value)[1].clone(),
        rules(&installed_value)[3].clone(),
    ];

    let removed = installed.remove(&PolicySpec::v1()).expect("remove Repose");
    let removed_value = parse_value(&removed.to_xml_bytes().expect("serialize removed"));

    assert_eq!(rules(&removed_value), expected_third_party);
    assert_eq!(removed.repose_candidate_index(), None);
    assert_eq!(removed.password_fallback_index(), Some(2));
}

#[test]
fn remove_preserves_a_third_party_candidate_added_after_install() {
    let installed = ScreenSaverPolicy::parse(&fixture("third-party.plist"))
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .expect("install Repose");
    let mut live_value = parse_value(&installed.to_xml_bytes().expect("serialize installed"));
    live_value
        .as_dictionary_mut()
        .and_then(|root| root.get_mut("rule"))
        .and_then(Value::as_array_mut)
        .expect("rule array")
        .insert(2, Value::String("com.example.added-later".to_owned()));
    let mut live_bytes = Vec::new();
    live_value
        .to_writer_xml(&mut live_bytes)
        .expect("serialize externally changed policy");

    let removed = ScreenSaverPolicy::parse(&live_bytes)
        .and_then(|policy| policy.remove(&PolicySpec::v1()))
        .expect("remove from current policy");
    let output = parse_value(&removed.to_xml_bytes().expect("serialize removed"));
    let names: Vec<_> = rules(&output)
        .iter()
        .map(|candidate| candidate.as_string().expect("string candidate"))
        .collect();

    assert_eq!(
        names,
        vec![
            "com.example.security-key",
            "com.example.offline-login",
            "com.example.added-later",
            "use-login-window-ui"
        ]
    );
}

#[test]
fn remove_is_idempotent_when_repose_is_absent() {
    let once = ScreenSaverPolicy::parse(&fixture("third-party.plist"))
        .and_then(|policy| policy.remove(&PolicySpec::v1()))
        .expect("safe uninstalled policy")
        .to_xml_bytes()
        .expect("serialize once");
    let twice = ScreenSaverPolicy::parse(&once)
        .and_then(|policy| policy.remove(&PolicySpec::v1()))
        .expect("remove twice")
        .to_xml_bytes()
        .expect("serialize twice");

    assert_eq!(once, twice);
}

#[test]
fn rejects_missing_password_fallback() {
    let error = ScreenSaverPolicy::parse(&fixture("missing-fallback.plist"))
        .expect_err("missing fallback must fail closed");
    assert_eq!(error, PolicyError::MissingPasswordFallback);
}

#[test]
fn rejects_duplicate_repose_candidates_on_install() {
    let error = ScreenSaverPolicy::parse(&fixture("duplicate-repose.plist"))
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .expect_err("duplicates are ambiguous");
    assert_eq!(error, PolicyError::DuplicateReposeCandidates { count: 2 });
}

#[test]
fn removal_recovers_from_duplicate_repose_candidates_surgically() {
    let removed = ScreenSaverPolicy::parse(&fixture("duplicate-repose.plist"))
        .and_then(|policy| policy.remove(&PolicySpec::v1()))
        .expect("removal only deletes owned candidates");

    assert_eq!(removed.repose_candidate_index(), None);
    assert!(removed.has_exactly_one_password_fallback());
    assert_eq!(
        rules(&parse_value(&removed.to_xml_bytes().unwrap())).len(),
        1
    );
}

#[test]
fn rejects_malformed_plist_without_panicking() {
    assert!(matches!(
        ScreenSaverPolicy::parse(&fixture("malformed.plist")),
        Err(PolicyError::MalformedPlist { .. })
    ));
}

#[test]
fn rejects_wrong_rule_class() {
    let error = ScreenSaverPolicy::parse(&fixture("wrong-class.plist"))
        .expect_err("wrong class cannot be transformed");
    assert_eq!(
        error,
        PolicyError::UnsupportedClass {
            actual: Some("evaluate-mechanisms".to_owned())
        }
    );
}

#[test]
fn reports_a_non_string_class_as_a_type_error() {
    let input = br#"<?xml version="1.0"?><plist version="1.0"><dict>
        <key>class</key><integer>1</integer>
        <key>rule</key><string>use-login-window-ui</string>
    </dict></plist>"#;

    assert_eq!(
        ScreenSaverPolicy::parse(input).unwrap_err(),
        PolicyError::InvalidFieldType {
            field: "class",
            expected: "string",
            actual: "integer",
        }
    );
}

#[test]
fn rejects_k_of_n_other_than_one() {
    let error = ScreenSaverPolicy::parse(&fixture("k-of-n-two.plist"))
        .expect_err("conjunctive policy would break fallback");
    assert_eq!(error, PolicyError::UnsafeThreshold { actual: Some(2) });
}

#[test]
fn rejects_multiple_candidates_without_an_explicit_or_threshold() {
    let error = ScreenSaverPolicy::parse(&fixture("multi-without-k-of-n.plist"))
        .expect_err("missing threshold must not silently become OR");
    assert_eq!(error, PolicyError::UnsafeThreshold { actual: None });
}

#[test]
fn policy_spec_has_no_arbitrary_right_input() {
    let spec = PolicySpec::v1();

    assert_eq!(spec.candidate_name(), "ai.repose.unlock");
    assert_eq!(
        ScreenSaverPolicy::SUPPORTED_RIGHT,
        "system.login.screensaver"
    );
}

#[test]
fn rejects_duplicate_password_fallbacks() {
    let error = ScreenSaverPolicy::parse(&fixture("duplicate-fallback.plist"))
        .expect_err("duplicate fallback is ambiguous");
    assert_eq!(error, PolicyError::DuplicatePasswordFallbacks { count: 2 });
}

#[test]
fn rejects_dictionary_candidates_instead_of_interpreting_them() {
    let error = ScreenSaverPolicy::parse(&fixture("dictionary-candidate.plist"))
        .expect_err("inline rules are outside the supported shape");
    assert_eq!(
        error,
        PolicyError::UnsupportedCandidateType {
            index: 0,
            actual: "dictionary"
        }
    );
}

#[test]
fn rejects_duplicate_dictionary_keys_before_plist_value_collapses_them() {
    let error = ScreenSaverPolicy::parse(&fixture("duplicate-root-key.plist"))
        .expect_err("duplicate keys are ambiguous");
    assert_eq!(
        error,
        PolicyError::DuplicateDictionaryKey {
            key: "rule".to_owned()
        }
    );
}

#[test]
fn preserves_binary_plist_encoding() {
    let xml = parse_value(&fixture("stock-string.plist"));
    let mut binary = Vec::new();
    xml.to_writer_binary(&mut binary)
        .expect("create binary fixture");

    let output = ScreenSaverPolicy::parse(&binary)
        .and_then(|policy| policy.install(&PolicySpec::v1()))
        .and_then(|policy| policy.to_bytes())
        .expect("transform binary policy");

    assert!(output.starts_with(b"bplist00"));
    let reparsed = parse_value(&output);
    assert_eq!(rules(&reparsed).len(), 2);
}

#[test]
fn rejects_ascii_plists_and_trailing_junk() {
    let ascii = br#"{ class = rule; rule = use-login-window-ui; }"#;
    assert_eq!(
        ScreenSaverPolicy::parse(ascii).unwrap_err(),
        PolicyError::UnsupportedEncoding
    );

    let mut xml_with_junk = fixture("stock-string.plist");
    xml_with_junk.extend_from_slice(b"not plist");
    assert!(ScreenSaverPolicy::parse(&xml_with_junk).is_err());
}

#[test]
fn rejects_oversized_input() {
    let input = vec![b' '; ScreenSaverPolicy::MAX_INPUT_BYTES + 1];
    assert_eq!(
        ScreenSaverPolicy::parse(&input).unwrap_err(),
        PolicyError::InputTooLarge {
            actual: ScreenSaverPolicy::MAX_INPUT_BYTES + 1,
            maximum: ScreenSaverPolicy::MAX_INPUT_BYTES,
        }
    );
}

#[test]
fn rejects_wrong_root_and_strict_field_type_matrix() {
    let cases = [
        (
            "<array><string>rule</string></array>",
            "root must be dictionary",
        ),
        (
            "<dict><key>rule</key><string>use-login-window-ui</string></dict>",
            "class must exist",
        ),
        (
            "<dict><key>class</key><integer>1</integer><key>rule</key><string>use-login-window-ui</string></dict>",
            "class must be a string",
        ),
        (
            "<dict><key>class</key><string>rule</string></dict>",
            "rule must exist",
        ),
        (
            "<dict><key>class</key><string>rule</string><key>rule</key><integer>1</integer></dict>",
            "rule must be string or string array",
        ),
        (
            "<dict><key>class</key><string>rule</string><key>k-of-n</key><real>1.0</real><key>rule</key><array><string>use-login-window-ui</string></array></dict>",
            "threshold must be integer",
        ),
        (
            "<dict><key>class</key><string>rule</string><key>k-of-n</key><integer>1</integer><key>rule</key><array><true/><string>use-login-window-ui</string></array></dict>",
            "candidate must be a string",
        ),
        (
            "<dict><key>class</key><string>rule</string><key>k-of-n</key><integer>1</integer><key>rule</key><array/></dict>",
            "empty rules cannot preserve fallback",
        ),
    ];

    for (body, reason) in cases {
        let input = format!("<?xml version=\"1.0\"?><plist version=\"1.0\">{body}</plist>");
        assert!(
            ScreenSaverPolicy::parse(input.as_bytes()).is_err(),
            "unexpectedly accepted case: {reason}"
        );
    }
}

#[test]
fn rejects_duplicate_keys_in_unknown_nested_dictionaries() {
    let input = br#"<?xml version="1.0"?><plist version="1.0"><dict>
        <key>class</key><string>rule</string>
        <key>rule</key><string>use-login-window-ui</string>
        <key>vendor</key><dict><key>mode</key><string>a</string><key>mode</key><string>b</string></dict>
    </dict></plist>"#;

    assert_eq!(
        ScreenSaverPolicy::parse(input).unwrap_err(),
        PolicyError::DuplicateDictionaryKey {
            key: "mode".to_owned()
        }
    );
}
