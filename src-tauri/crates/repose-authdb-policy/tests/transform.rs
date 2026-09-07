use plist::{Date, Dictionary, Integer, Uid, Value};
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

fn binary_length_prefix(kind: u8, length: usize) -> Vec<u8> {
    let mut object = Vec::new();
    match length {
        length @ 0..=14 => object.push(kind | u8::try_from(length).unwrap()),
        length @ 15..=255 => {
            object.extend_from_slice(&[kind | 0x0f, 0x10, u8::try_from(length).unwrap()]);
        }
        length @ 256..=65535 => {
            object.extend_from_slice(&[kind | 0x0f, 0x11]);
            object.extend_from_slice(&u16::try_from(length).unwrap().to_be_bytes());
        }
        length => {
            object.extend_from_slice(&[kind | 0x0f, 0x12]);
            object.extend_from_slice(&u32::try_from(length).unwrap().to_be_bytes());
        }
    }
    object
}

fn binary_sized_object(kind: u8, bytes: &[u8]) -> Vec<u8> {
    let mut object = binary_length_prefix(kind, bytes.len());
    object.extend_from_slice(bytes);
    object
}

fn binary_collection_object(kind: u8, count: usize, references: &[u8]) -> Vec<u8> {
    let mut object = binary_length_prefix(kind, count);
    object.extend_from_slice(references);
    object
}

fn binary_string(value: &str) -> Vec<u8> {
    binary_sized_object(0x50, value.as_bytes())
}

fn binary_string_with_u64_length_marker(marker: u8, value: &str) -> Vec<u8> {
    let mut object = vec![0x5f, marker];
    object.extend_from_slice(&u64::try_from(value.len()).unwrap().to_be_bytes());
    object.extend_from_slice(value.as_bytes());
    object
}

fn shared_binary_policy(leaf: Vec<u8>, depth: usize) -> Vec<u8> {
    assert!(depth < 200, "one-byte object references are required");
    let mut objects = vec![
        Vec::new(),
        binary_string("class"),
        binary_string("rule"),
        binary_string("use-login-window-ui"),
        binary_string("vendor"),
        leaf,
    ];
    let mut shared_ref = 5u8;
    for _ in 0..depth {
        objects.push(vec![0xa2, shared_ref, shared_ref]);
        shared_ref = u8::try_from(objects.len() - 1).unwrap();
    }
    objects[0] = vec![0xd3, 1, 2, 4, 2, 3, shared_ref];

    finish_binary_objects(objects)
}

fn finish_binary_objects(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut output = b"bplist00".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for object in &objects {
        offsets.push(u64::try_from(output.len()).unwrap());
        output.extend_from_slice(object);
    }
    let offset_table_offset = u64::try_from(output.len()).unwrap();
    let offset_size = if output.len() <= usize::from(u8::MAX) {
        1u8
    } else if output.len() <= usize::from(u16::MAX) {
        2u8
    } else {
        4u8
    };
    for offset in offsets {
        let encoded = offset.to_be_bytes();
        output.extend_from_slice(&encoded[encoded.len() - usize::from(offset_size)..]);
    }
    output.extend_from_slice(&[0; 6]);
    output.push(offset_size);
    output.push(1);
    output.extend_from_slice(&u64::try_from(objects.len()).unwrap().to_be_bytes());
    output.extend_from_slice(&0u64.to_be_bytes());
    output.extend_from_slice(&offset_table_offset.to_be_bytes());
    output
}

fn encode_binary_uint(value: usize, width: usize) -> Vec<u8> {
    let encoded = u64::try_from(value).unwrap().to_be_bytes();
    encoded[encoded.len() - width..].to_vec()
}

fn finish_binary_objects_with_layout(
    objects: &[Vec<u8>],
    physical_order: &[usize],
    root_object: usize,
    offset_width: usize,
    reference_width: usize,
) -> Vec<u8> {
    assert_eq!(objects.len(), physical_order.len());
    let mut output = b"bplist00".to_vec();
    let mut offsets = vec![None; objects.len()];
    for &object_index in physical_order {
        assert!(offsets[object_index].is_none());
        offsets[object_index] = Some(output.len());
        output.extend_from_slice(&objects[object_index]);
    }
    let offset_table_offset = output.len();
    for offset in offsets {
        output.extend_from_slice(&encode_binary_uint(offset.unwrap(), offset_width));
    }
    output.extend_from_slice(&[0; 6]);
    output.push(u8::try_from(offset_width).unwrap());
    output.push(u8::try_from(reference_width).unwrap());
    output.extend_from_slice(&u64::try_from(objects.len()).unwrap().to_be_bytes());
    output.extend_from_slice(&u64::try_from(root_object).unwrap().to_be_bytes());
    output.extend_from_slice(&u64::try_from(offset_table_offset).unwrap().to_be_bytes());
    output
}

fn wide_binary_array_policy(count: usize) -> Vec<u8> {
    let references = vec![6; count];
    finish_binary_objects(vec![
        vec![0xd3, 1, 2, 4, 2, 3, 5],
        binary_string("class"),
        binary_string("rule"),
        binary_string("use-login-window-ui"),
        binary_string("vendor"),
        binary_collection_object(0xa0, count, &references),
        vec![0x09],
    ])
}

fn wide_binary_dictionary_policy(count: usize) -> Vec<u8> {
    let mut references = vec![6; count];
    references.extend(std::iter::repeat_n(7, count));
    finish_binary_objects(vec![
        vec![0xd3, 1, 2, 4, 2, 3, 5],
        binary_string("class"),
        binary_string("rule"),
        binary_string("use-login-window-ui"),
        binary_string("vendor"),
        binary_collection_object(0xd0, count, &references),
        binary_string("duplicate-key"),
        vec![0x09],
    ])
}

fn valid_binary_policy() -> Vec<u8> {
    let value = parse_value(&fixture("stock-string.plist"));
    let mut binary = Vec::new();
    value
        .to_writer_binary(&mut binary)
        .expect("serialize binary policy");
    binary
}

fn trailer_offset_table_start(binary: &[u8]) -> usize {
    usize::try_from(u64::from_be_bytes(
        binary[binary.len() - 8..].try_into().unwrap(),
    ))
    .unwrap()
}

#[test]
fn rejects_trailing_bytes_hidden_before_a_copied_valid_trailer() {
    let valid = valid_binary_policy();
    let trailer = valid[valid.len() - 32..].to_vec();
    let mut bypass = valid;
    bypass.extend_from_slice(b"ignored trailing bytes and an old layout");
    bypass.extend_from_slice(&trailer);

    assert!(matches!(
        ScreenSaverPolicy::parse(&bypass),
        Err(PolicyError::InvalidBinaryLayout { .. })
    ));
}

#[test]
fn rejects_holes_hidden_by_a_relocated_contiguous_offset_table() {
    let valid = valid_binary_policy();
    let old_trailer_start = valid.len() - 32;
    let old_offset_table_start = trailer_offset_table_start(&valid);
    let old_offset_table = valid[old_offset_table_start..old_trailer_start].to_vec();
    let mut final_trailer = valid[old_trailer_start..].to_vec();

    let mut bypass = valid[..old_trailer_start].to_vec();
    bypass.extend_from_slice(b"unreferenced hole");
    let new_offset_table_start = u64::try_from(bypass.len()).unwrap();
    bypass.extend_from_slice(&old_offset_table);
    final_trailer[24..32].copy_from_slice(&new_offset_table_start.to_be_bytes());
    bypass.extend_from_slice(&final_trailer);

    assert!(matches!(
        ScreenSaverPolicy::parse(&bypass),
        Err(PolicyError::InvalidBinaryLayout { .. })
    ));
}

#[test]
fn rejects_wide_collections_before_plist_reader_allocates_reference_vectors() {
    let array_count = ScreenSaverPolicy::MAX_BINARY_ARRAY_ITEMS + 1;
    let wide_array = wide_binary_array_policy(array_count);

    assert!(wide_array.len() < 32 * 1024);
    assert_eq!(
        ScreenSaverPolicy::parse(&wide_array).unwrap_err(),
        PolicyError::BinaryCollectionLimitExceeded {
            kind: "array",
            declared: array_count,
            maximum: ScreenSaverPolicy::MAX_BINARY_ARRAY_ITEMS,
        }
    );

    let dictionary_count = ScreenSaverPolicy::MAX_BINARY_DICTIONARY_ITEMS + 1;
    let wide_dictionary = wide_binary_dictionary_policy(dictionary_count);
    assert!(wide_dictionary.len() < 32 * 1024);
    assert_eq!(
        ScreenSaverPolicy::parse(&wide_dictionary).unwrap_err(),
        PolicyError::BinaryCollectionLimitExceeded {
            kind: "dictionary",
            declared: dictionary_count,
            maximum: ScreenSaverPolicy::MAX_BINARY_DICTIONARY_ITEMS,
        }
    );
}

#[test]
fn rejects_excessive_unreachable_or_cyclic_binary_objects_before_generic_parsing() {
    let mut excessive_count = valid_binary_policy();
    let trailer_start = excessive_count.len() - 32;
    excessive_count[trailer_start + 8..trailer_start + 16].copy_from_slice(
        &u64::try_from(ScreenSaverPolicy::MAX_BINARY_OBJECTS + 1)
            .unwrap()
            .to_be_bytes(),
    );
    assert_eq!(
        ScreenSaverPolicy::parse(&excessive_count).unwrap_err(),
        PolicyError::BinaryObjectLimitExceeded {
            declared: ScreenSaverPolicy::MAX_BINARY_OBJECTS + 1,
            maximum: ScreenSaverPolicy::MAX_BINARY_OBJECTS,
        }
    );

    let unreachable = finish_binary_objects(vec![
        vec![0xd2, 1, 2, 2, 3],
        binary_string("class"),
        binary_string("rule"),
        binary_string("use-login-window-ui"),
        vec![0x09],
    ]);
    assert_eq!(
        ScreenSaverPolicy::parse(&unreachable).unwrap_err(),
        PolicyError::InvalidBinaryLayout {
            reason: "binary object graph contains unreachable objects",
        }
    );

    let cyclic = finish_binary_objects(vec![
        vec![0xd3, 1, 2, 4, 2, 3, 5],
        binary_string("class"),
        binary_string("rule"),
        binary_string("use-login-window-ui"),
        binary_string("vendor"),
        vec![0xa1, 5],
    ]);
    assert_eq!(
        ScreenSaverPolicy::parse(&cyclic).unwrap_err(),
        PolicyError::InvalidBinaryLayout {
            reason: "binary object graph contains a cycle",
        }
    );
}

#[test]
fn rejects_invalid_references_indirect_cycles_and_ambiguous_length_markers() {
    let invalid_reference = finish_binary_objects(vec![
        vec![0xd3, 1, 2, 4, 2, 3, 5],
        binary_string("class"),
        binary_string("rule"),
        binary_string("use-login-window-ui"),
        binary_string("vendor"),
        vec![0xa1, 6],
    ]);
    let indirect_cycle = finish_binary_objects(vec![
        vec![0xd3, 1, 2, 4, 2, 3, 5],
        binary_string("class"),
        binary_string("rule"),
        binary_string("use-login-window-ui"),
        binary_string("vendor"),
        vec![0xa1, 6],
        vec![0xa1, 5],
    ]);
    let ambiguous_length = finish_binary_objects(vec![
        vec![0xd2, 1, 2, 2, 3],
        binary_string("class"),
        binary_string("rule"),
        binary_string_with_u64_length_marker(0xff, "use-login-window-ui"),
    ]);

    for (input, reason) in [
        (invalid_reference, "collection reference is out of range"),
        (indirect_cycle, "binary object graph contains a cycle"),
        (ambiguous_length, "invalid extended-length integer marker"),
    ] {
        assert_eq!(
            ScreenSaverPolicy::parse(&input).unwrap_err(),
            PolicyError::InvalidBinaryLayout { reason }
        );
    }
}

#[test]
fn accepts_writer_binary_with_every_supported_scalar_and_nested_unknown_values() {
    let mut expected = parse_value(&fixture("stock-string.plist"));
    let mut nested = Dictionary::new();
    nested.insert("data".to_owned(), Value::Data(vec![0, 1, 2, 0xff]));
    nested.insert(
        "date".to_owned(),
        Value::Date(Date::from_xml_format("2026-09-07T12:34:56Z").unwrap()),
    );
    nested.insert("uid".to_owned(), Value::Uid(Uid::new(u64::MAX)));
    nested.insert(
        "uid-widths".to_owned(),
        Value::Array(
            [
                u64::from(u8::MAX),
                u64::from(u8::MAX) + 1,
                u64::from(u16::MAX),
                u64::from(u16::MAX) + 1,
                u64::from(u32::MAX),
                u64::from(u32::MAX) + 1,
            ]
            .into_iter()
            .map(|value| Value::Uid(Uid::new(value)))
            .collect(),
        ),
    );
    nested.insert(
        "values".to_owned(),
        Value::Array(vec![
            Value::Boolean(false),
            Value::Integer(i64::MIN.into()),
            Value::Integer(u64::MAX.into()),
            Value::Real(3.5),
            Value::String("距离😀校准".to_owned()),
        ]),
    );
    expected
        .as_dictionary_mut()
        .unwrap()
        .insert("vendor-unknown".to_owned(), Value::Dictionary(nested));

    let mut binary = Vec::new();
    expected.to_writer_binary(&mut binary).unwrap();
    let output = ScreenSaverPolicy::parse(&binary)
        .and_then(|policy| policy.to_bytes())
        .expect("canonical writer output must round-trip");

    assert_eq!(parse_value(&output), expected);
}

#[test]
fn accepts_writer_binary_at_sized_object_and_collection_boundaries() {
    for length in [14, 15, 255, 256, 65_535, 65_536] {
        let mut expected = parse_value(&fixture("stock-string.plist"));
        expected
            .as_dictionary_mut()
            .unwrap()
            .insert("vendor-data".to_owned(), Value::Data(vec![0xa5; length]));
        let mut binary = Vec::new();
        expected.to_writer_binary(&mut binary).unwrap();

        let parsed = ScreenSaverPolicy::parse(&binary).unwrap();
        assert_eq!(parse_value(&parsed.to_bytes().unwrap()), expected);
    }

    for length in [14, 15, 255, 256] {
        let mut expected = parse_value(&fixture("stock-string.plist"));
        expected.as_dictionary_mut().unwrap().insert(
            "vendor-array".to_owned(),
            Value::Array(vec![Value::Boolean(true); length]),
        );
        let mut binary = Vec::new();
        expected.to_writer_binary(&mut binary).unwrap();

        let parsed = ScreenSaverPolicy::parse(&binary).unwrap();
        assert_eq!(parse_value(&parsed.to_bytes().unwrap()), expected);
    }
}

#[test]
fn accepts_all_valid_table_widths_nonzero_root_and_nonphysical_offset_order() {
    for offset_width in [1, 2, 3, 4, 8] {
        for reference_width in [1, 2, 3, 4, 8] {
            let references = [0, 1, 1, 2]
                .into_iter()
                .flat_map(|reference| encode_binary_uint(reference, reference_width))
                .collect::<Vec<_>>();
            let objects = vec![
                binary_string("class"),
                binary_string("rule"),
                binary_string("use-login-window-ui"),
                binary_collection_object(0xd0, 2, &references),
            ];
            let binary = finish_binary_objects_with_layout(
                &objects,
                &[2, 0, 3, 1],
                3,
                offset_width,
                reference_width,
            );

            let policy = ScreenSaverPolicy::parse(&binary).unwrap_or_else(|error| {
                panic!("valid offset width {offset_width}, ref width {reference_width}: {error}")
            });
            assert_eq!(policy.password_fallback_index(), Some(0));
        }
    }
}

#[test]
fn accepts_nonminimal_extended_length_width() {
    let binary = finish_binary_objects(vec![
        vec![0xd2, 1, 2, 2, 3],
        binary_string("class"),
        binary_string("rule"),
        binary_string_with_u64_length_marker(0x13, "use-login-window-ui"),
    ]);

    let policy = ScreenSaverPolicy::parse(&binary).expect("nonminimal length width is legal");
    assert_eq!(policy.password_fallback_index(), Some(0));
}

fn policy_with_threshold(threshold: Integer, binary: bool) -> Vec<u8> {
    let mut policy = parse_value(&fixture("stock-array.plist"));
    policy
        .as_dictionary_mut()
        .expect("root dictionary")
        .insert("k-of-n".to_owned(), Value::Integer(threshold));
    let mut output = Vec::new();
    if binary {
        policy
            .to_writer_binary(&mut output)
            .expect("serialize binary threshold policy");
    } else {
        policy
            .to_writer_xml(&mut output)
            .expect("serialize XML threshold policy");
    }
    output
}

#[test]
fn rejects_compact_binary_dag_that_expands_to_too_many_events() {
    let compact_exponential_policy = shared_binary_policy(vec![0x09], 13);

    assert!(compact_exponential_policy.len() < 1024);
    assert_eq!(
        ScreenSaverPolicy::parse(&compact_exponential_policy).unwrap_err(),
        PolicyError::ExpandedEventLimitExceeded {
            maximum: ScreenSaverPolicy::MAX_EXPANDED_EVENTS,
        }
    );
}

#[test]
fn rejects_compact_binary_dag_that_repeats_too_many_data_bytes() {
    let data = vec![0xa5; 32 * 1024];
    let shared_data_policy = shared_binary_policy(binary_sized_object(0x40, &data), 4);

    assert!(shared_data_policy.len() < 33 * 1024);
    assert_eq!(
        ScreenSaverPolicy::parse(&shared_data_policy).unwrap_err(),
        PolicyError::ExpandedByteLimitExceeded {
            maximum: ScreenSaverPolicy::MAX_EXPANDED_SCALAR_BYTES,
        }
    );
}

#[test]
fn applies_expansion_limits_to_xml_and_accepts_ordinary_bounded_values() {
    let many_values = "<true/>".repeat(ScreenSaverPolicy::MAX_EXPANDED_EVENTS + 1);
    let event_heavy_xml = format!(
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\
         <key>class</key><string>rule</string>\
         <key>rule</key><string>use-login-window-ui</string>\
         <key>vendor</key><array>{many_values}</array>\
         </dict></plist>"
    );
    assert_eq!(
        ScreenSaverPolicy::parse(event_heavy_xml.as_bytes()).unwrap_err(),
        PolicyError::ExpandedEventLimitExceeded {
            maximum: ScreenSaverPolicy::MAX_EXPANDED_EVENTS,
        }
    );

    // Root dictionary, five key/value strings, the vendor array, and both
    // collection end events account for nine events beyond its booleans.
    let boundary_values = "<true/>".repeat(ScreenSaverPolicy::MAX_EXPANDED_EVENTS - 9);
    let event_boundary_xml = format!(
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\
         <key>class</key><string>rule</string>\
         <key>rule</key><string>use-login-window-ui</string>\
         <key>vendor</key><array>{boundary_values}</array>\
         </dict></plist>"
    );
    assert!(ScreenSaverPolicy::parse(event_boundary_xml.as_bytes()).is_ok());

    let scalar_heavy_xml = format!(
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\
         <key>class</key><string>rule</string>\
         <key>rule</key><string>use-login-window-ui</string>\
         <key>vendor</key><string>{}</string>\
         </dict></plist>",
        "a".repeat(ScreenSaverPolicy::MAX_EXPANDED_SCALAR_BYTES)
    );
    assert_eq!(
        ScreenSaverPolicy::parse(scalar_heavy_xml.as_bytes()).unwrap_err(),
        PolicyError::ExpandedByteLimitExceeded {
            maximum: ScreenSaverPolicy::MAX_EXPANDED_SCALAR_BYTES,
        }
    );

    // The five required key/value strings contain 38 bytes in total.
    let scalar_boundary_xml = format!(
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\
         <key>class</key><string>rule</string>\
         <key>rule</key><string>use-login-window-ui</string>\
         <key>vendor</key><string>{}</string>\
         </dict></plist>",
        "a".repeat(ScreenSaverPolicy::MAX_EXPANDED_SCALAR_BYTES - 38)
    );
    assert!(ScreenSaverPolicy::parse(scalar_boundary_xml.as_bytes()).is_ok());

    assert!(ScreenSaverPolicy::parse(&shared_binary_policy(vec![0x09], 3)).is_ok());
    assert!(ScreenSaverPolicy::parse(&fixture("stock-string.plist")).is_ok());
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
fn verify_installed_directly_checks_presence_uniqueness_and_adjacency() {
    let spec = PolicySpec::v1();
    ScreenSaverPolicy::parse(&fixture("already-installed.plist"))
        .expect("canonical policy")
        .verify_installed(&spec)
        .expect("canonical policy verifies");

    let missing = ScreenSaverPolicy::parse(&fixture("third-party.plist"))
        .expect("valid policy without Repose")
        .verify_installed(&spec)
        .expect_err("missing Repose must not verify");
    assert_eq!(missing, PolicyError::MissingReposeCandidate);

    let misplaced = ScreenSaverPolicy::parse(&fixture("misplaced-repose.plist"))
        .expect("valid policy with misplaced Repose")
        .verify_installed(&spec)
        .expect_err("misplaced Repose must not verify");
    assert_eq!(
        misplaced,
        PolicyError::MisplacedReposeCandidate {
            repose_index: 0,
            fallback_index: 2,
        }
    );

    let duplicate = ScreenSaverPolicy::parse(&fixture("duplicate-repose.plist"))
        .expect("valid policy with duplicate Repose")
        .verify_installed(&spec)
        .expect_err("duplicate Repose must not verify");
    assert_eq!(
        duplicate,
        PolicyError::DuplicateReposeCandidates { count: 2 }
    );
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
    let mut expected = live_value.clone();
    expected
        .as_dictionary_mut()
        .and_then(|root| root.get_mut("rule"))
        .and_then(Value::as_array_mut)
        .expect("expected rule array")
        .retain(|candidate| candidate.as_string() != Some("ai.repose.unlock"));
    let mut live_bytes = Vec::new();
    live_value
        .to_writer_xml(&mut live_bytes)
        .expect("serialize externally changed policy");

    let removed = ScreenSaverPolicy::parse(&live_bytes)
        .and_then(|policy| policy.remove(&PolicySpec::v1()))
        .expect("remove from current policy");
    let output = parse_value(&removed.to_xml_bytes().expect("serialize removed"));

    assert_eq!(output, expected);
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
    let input = fixture("duplicate-repose.plist");
    let mut expected = parse_value(&input);
    expected
        .as_dictionary_mut()
        .and_then(|root| root.get_mut("rule"))
        .and_then(Value::as_array_mut)
        .expect("expected rule array")
        .retain(|candidate| candidate.as_string() != Some("ai.repose.unlock"));
    let removed = ScreenSaverPolicy::parse(&input)
        .and_then(|policy| policy.remove(&PolicySpec::v1()))
        .expect("removal only deletes owned candidates");
    let output = parse_value(&removed.to_xml_bytes().expect("serialize removed"));

    assert_eq!(removed.repose_candidate_index(), None);
    assert!(removed.has_exactly_one_password_fallback());
    assert_eq!(output, expected);
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
fn rejects_every_signed_threshold_boundary_other_than_one() {
    for threshold in [0, -1, 2, i64::MIN, i64::MAX] {
        let error = ScreenSaverPolicy::parse(&policy_with_threshold(threshold.into(), false))
            .expect_err("only OR threshold one is supported");
        assert_eq!(
            error,
            PolicyError::UnsafeThreshold {
                actual: Some(threshold)
            },
            "threshold {threshold}"
        );
    }
}

#[test]
fn rejects_binary_unsigned_threshold_beyond_i64() {
    let error = ScreenSaverPolicy::parse(&policy_with_threshold(u64::MAX.into(), true))
        .expect_err("out-of-range unsigned threshold must fail closed");
    assert_eq!(error, PolicyError::UnsafeThreshold { actual: None });
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
fn rejects_binary_plist_with_trailing_junk() {
    let value = parse_value(&fixture("stock-string.plist"));
    let mut binary = Vec::new();
    value
        .to_writer_binary(&mut binary)
        .expect("serialize binary policy");
    binary.extend_from_slice(b"trailing junk");

    assert!(matches!(
        ScreenSaverPolicy::parse(&binary),
        Err(PolicyError::InvalidBinaryLayout { .. })
    ));
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
