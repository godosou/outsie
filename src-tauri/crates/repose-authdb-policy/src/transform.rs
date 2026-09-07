use std::{collections::HashSet, fmt, io::Cursor};

use plist::{
    Dictionary, Value,
    stream::{Event, Reader},
};

const PASSWORD_FALLBACK: &str = "use-login-window-ui";
const REPOSE_V1_CANDIDATE: &str = "ai.repose.unlock";

/// The fixed, versioned policy fragment understood by this crate.
///
/// There is intentionally no public constructor and no authorization-right or
/// mechanism input. Future versions must be added as audited constructors.
///
/// ```compile_fail
/// use repose_authdb_policy::PolicySpec;
///
/// let _ = PolicySpec::for_right("system.login.console");
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicySpec {
    _private: (),
}

impl PolicySpec {
    #[must_use]
    pub const fn v1() -> Self {
        Self { _private: () }
    }

    #[must_use]
    pub const fn candidate_name(&self) -> &'static str {
        REPOSE_V1_CANDIDATE
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Encoding {
    Xml,
    Binary,
}

/// A validated, in-memory `system.login.screensaver` policy.
///
/// Parsing and transforming are pure. Calling these methods cannot access the
/// macOS authorization database.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenSaverPolicy {
    root: Dictionary,
    encoding: Encoding,
}

impl ScreenSaverPolicy {
    pub const SUPPORTED_RIGHT: &'static str = "system.login.screensaver";
    pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
    /// Maximum number of events after binary shared objects are expanded.
    ///
    /// Real screensaver rules are tiny. This generous bound prevents a compact
    /// binary plist DAG from causing exponential work or allocations.
    pub const MAX_EXPANDED_EVENTS: usize = 16 * 1024;
    /// Maximum cumulative bytes across expanded keys, strings, data, and
    /// fixed-width scalar values.
    pub const MAX_EXPANDED_SCALAR_BYTES: usize = 256 * 1024;
    const MAX_NESTING_DEPTH: usize = 64;

    pub fn parse(input: &[u8]) -> Result<Self, PolicyError> {
        if input.len() > Self::MAX_INPUT_BYTES {
            return Err(PolicyError::InputTooLarge {
                actual: input.len(),
                maximum: Self::MAX_INPUT_BYTES,
            });
        }

        let (encoding, payload) = detect_encoding(input)?;
        preflight_event_stream(
            payload,
            Self::MAX_NESTING_DEPTH,
            Self::MAX_EXPANDED_EVENTS,
            Self::MAX_EXPANDED_SCALAR_BYTES,
        )?;
        let value = Value::from_reader(Cursor::new(payload)).map_err(malformed)?;
        let root = match value {
            Value::Dictionary(root) => root,
            other => {
                return Err(PolicyError::RootNotDictionary {
                    actual: value_type(&other),
                });
            }
        };

        let policy = Self { root, encoding };
        policy.validate_shape()?;
        Ok(policy)
    }

    pub fn install(mut self, spec: &PolicySpec) -> Result<Self, PolicyError> {
        self.validate_shape()?;

        let (repose_count, repose_index, fallback_index) = self.candidate_positions(spec)?;
        match repose_count {
            0 => self.insert_repose_candidate(spec, fallback_index)?,
            1 if repose_index.is_some_and(|index| index + 1 == fallback_index) => {}
            1 => self
                .move_repose_candidate(repose_index.ok_or(PolicyError::MissingReposeCandidate)?)?,
            count => return Err(PolicyError::DuplicateReposeCandidates { count }),
        }

        self.verify_installed(spec)?;
        Ok(self)
    }

    pub fn remove(mut self, spec: &PolicySpec) -> Result<Self, PolicyError> {
        self.validate_shape()?;
        if let Some(rules) = self.rule_array_mut() {
            rules.retain(|candidate| candidate.as_string() != Some(spec.candidate_name()));
        }
        self.validate_shape()?;
        Ok(self)
    }

    pub fn verify_installed(&self, spec: &PolicySpec) -> Result<(), PolicyError> {
        self.validate_shape()?;
        let (count, repose_index, fallback_index) = self.candidate_positions(spec)?;
        if count != 1 {
            return Err(if count == 0 {
                PolicyError::MissingReposeCandidate
            } else {
                PolicyError::DuplicateReposeCandidates { count }
            });
        }
        let repose_index = repose_index.ok_or(PolicyError::MissingReposeCandidate)?;
        if repose_index + 1 != fallback_index {
            return Err(PolicyError::MisplacedReposeCandidate {
                repose_index,
                fallback_index,
            });
        }
        if self.threshold() != Some(1) {
            return Err(PolicyError::UnsafeThreshold {
                actual: self.threshold(),
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn has_exactly_one_password_fallback(&self) -> bool {
        self.candidate_strings()
            .map(|rules| {
                rules
                    .into_iter()
                    .filter(|candidate| *candidate == PASSWORD_FALLBACK)
                    .count()
                    == 1
            })
            .unwrap_or(false)
    }

    #[must_use]
    pub fn repose_candidate_index(&self) -> Option<usize> {
        self.candidate_strings().ok().and_then(|rules| {
            rules
                .iter()
                .position(|candidate| *candidate == REPOSE_V1_CANDIDATE)
        })
    }

    #[must_use]
    pub fn password_fallback_index(&self) -> Option<usize> {
        self.candidate_strings().ok().and_then(|rules| {
            rules
                .iter()
                .position(|candidate| *candidate == PASSWORD_FALLBACK)
        })
    }

    /// Serializes in the same plist encoding (XML or binary) as the input.
    ///
    /// The policy is semantically stable, but XML whitespace, comments, and
    /// binary object-table layout are not byte-preservation contracts.
    pub fn to_bytes(&self) -> Result<Vec<u8>, PolicyError> {
        let value = Value::Dictionary(self.root.clone());
        let mut output = Vec::new();
        match self.encoding {
            Encoding::Xml => value.to_writer_xml(&mut output),
            Encoding::Binary => value.to_writer_binary(&mut output),
        }
        .map_err(serialization)?;
        Ok(output)
    }

    /// Serializes as XML regardless of the input encoding.
    ///
    /// Binary-only plist values such as UIDs produce a serialization error.
    pub fn to_xml_bytes(&self) -> Result<Vec<u8>, PolicyError> {
        let mut output = Vec::new();
        Value::Dictionary(self.root.clone())
            .to_writer_xml(&mut output)
            .map_err(serialization)?;
        Ok(output)
    }

    fn validate_shape(&self) -> Result<(), PolicyError> {
        let actual_class = match self.root.get("class") {
            Some(Value::String(class)) => Some(class.clone()),
            Some(other) => {
                return Err(PolicyError::InvalidFieldType {
                    field: "class",
                    expected: "string",
                    actual: value_type(other),
                });
            }
            None => None,
        };
        if actual_class.as_deref() != Some("rule") {
            return Err(PolicyError::UnsupportedClass {
                actual: actual_class,
            });
        }

        let candidates = self.candidate_strings()?;
        let fallback_count = candidates
            .iter()
            .filter(|candidate| **candidate == PASSWORD_FALLBACK)
            .count();
        match fallback_count {
            0 => return Err(PolicyError::MissingPasswordFallback),
            1 => {}
            count => return Err(PolicyError::DuplicatePasswordFallbacks { count }),
        }

        match self.root.get("k-of-n") {
            Some(Value::Integer(value)) if value.as_signed() == Some(1) => {}
            Some(Value::Integer(value)) => {
                return Err(PolicyError::UnsafeThreshold {
                    actual: value.as_signed(),
                });
            }
            Some(other) => {
                return Err(PolicyError::InvalidFieldType {
                    field: "k-of-n",
                    expected: "integer",
                    actual: value_type(other),
                });
            }
            None if candidates.len() == 1 && candidates[0] == PASSWORD_FALLBACK => {}
            None => return Err(PolicyError::UnsafeThreshold { actual: None }),
        }

        Ok(())
    }

    fn candidate_strings(&self) -> Result<Vec<&str>, PolicyError> {
        let rule = self.root.get("rule").ok_or(PolicyError::MissingRule)?;
        match rule {
            Value::String(candidate) => Ok(vec![candidate.as_str()]),
            Value::Array(candidates) => candidates
                .iter()
                .enumerate()
                .map(|(index, candidate)| {
                    candidate
                        .as_string()
                        .ok_or(PolicyError::UnsupportedCandidateType {
                            index,
                            actual: value_type(candidate),
                        })
                })
                .collect(),
            other => Err(PolicyError::InvalidFieldType {
                field: "rule",
                expected: "string or array of strings",
                actual: value_type(other),
            }),
        }
    }

    fn candidate_positions(
        &self,
        spec: &PolicySpec,
    ) -> Result<(usize, Option<usize>, usize), PolicyError> {
        let candidates = self.candidate_strings()?;
        let mut repose_count = 0;
        let mut repose_index = None;
        let mut fallback_index = None;
        for (index, candidate) in candidates.into_iter().enumerate() {
            if candidate == spec.candidate_name() {
                repose_count += 1;
                repose_index.get_or_insert(index);
            }
            if candidate == PASSWORD_FALLBACK {
                fallback_index = Some(index);
            }
        }
        Ok((
            repose_count,
            repose_index,
            fallback_index.ok_or(PolicyError::MissingPasswordFallback)?,
        ))
    }

    fn insert_repose_candidate(
        &mut self,
        spec: &PolicySpec,
        fallback_index: usize,
    ) -> Result<(), PolicyError> {
        let rule = self.root.get_mut("rule").ok_or(PolicyError::MissingRule)?;
        match rule {
            Value::String(fallback) => {
                let fallback = std::mem::take(fallback);
                *rule = Value::Array(vec![
                    Value::String(spec.candidate_name().to_owned()),
                    Value::String(fallback),
                ]);
            }
            Value::Array(candidates) => candidates.insert(
                fallback_index,
                Value::String(spec.candidate_name().to_owned()),
            ),
            other => {
                return Err(PolicyError::InvalidFieldType {
                    field: "rule",
                    expected: "string or array of strings",
                    actual: value_type(other),
                });
            }
        }
        self.root
            .insert("k-of-n".to_owned(), Value::Integer(1.into()));
        Ok(())
    }

    fn rule_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        self.root.get_mut("rule").and_then(Value::as_array_mut)
    }

    fn move_repose_candidate(&mut self, repose_index: usize) -> Result<(), PolicyError> {
        let candidates = self.rule_array_mut().ok_or(PolicyError::MissingRule)?;
        let repose = candidates.remove(repose_index);
        let fallback_index = candidates
            .iter()
            .position(|candidate| candidate.as_string() == Some(PASSWORD_FALLBACK))
            .ok_or(PolicyError::MissingPasswordFallback)?;
        candidates.insert(fallback_index, repose);
        Ok(())
    }

    fn threshold(&self) -> Option<i64> {
        match self.root.get("k-of-n") {
            Some(Value::Integer(value)) => value.as_signed(),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyError {
    InputTooLarge {
        actual: usize,
        maximum: usize,
    },
    UnsupportedEncoding,
    MalformedPlist {
        detail: String,
    },
    DuplicateDictionaryKey {
        key: String,
    },
    NestingTooDeep {
        maximum: usize,
    },
    ExpandedEventLimitExceeded {
        maximum: usize,
    },
    ExpandedByteLimitExceeded {
        maximum: usize,
    },
    RootNotDictionary {
        actual: &'static str,
    },
    UnsupportedClass {
        actual: Option<String>,
    },
    MissingRule,
    InvalidFieldType {
        field: &'static str,
        expected: &'static str,
        actual: &'static str,
    },
    UnsupportedCandidateType {
        index: usize,
        actual: &'static str,
    },
    UnsafeThreshold {
        actual: Option<i64>,
    },
    MissingPasswordFallback,
    DuplicatePasswordFallbacks {
        count: usize,
    },
    MissingReposeCandidate,
    DuplicateReposeCandidates {
        count: usize,
    },
    MisplacedReposeCandidate {
        repose_index: usize,
        fallback_index: usize,
    },
    Serialization {
        detail: String,
    },
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge { actual, maximum } => {
                write!(formatter, "policy is {actual} bytes; maximum is {maximum}")
            }
            Self::UnsupportedEncoding => {
                formatter.write_str("policy must be an XML or binary property list")
            }
            Self::MalformedPlist { detail } => {
                write!(formatter, "malformed policy plist: {detail}")
            }
            Self::DuplicateDictionaryKey { key } => {
                write!(
                    formatter,
                    "policy contains duplicate dictionary key `{key}`"
                )
            }
            Self::NestingTooDeep { maximum } => {
                write!(formatter, "policy nesting exceeds {maximum} collections")
            }
            Self::ExpandedEventLimitExceeded { maximum } => write!(
                formatter,
                "expanded policy exceeds the {maximum}-event limit"
            ),
            Self::ExpandedByteLimitExceeded { maximum } => write!(
                formatter,
                "expanded policy exceeds the {maximum}-byte scalar/data limit"
            ),
            Self::RootNotDictionary { actual } => {
                write!(
                    formatter,
                    "policy root must be a dictionary, found {actual}"
                )
            }
            Self::UnsupportedClass { actual } => write!(
                formatter,
                "policy class must be `rule`, found {}",
                actual.as_deref().unwrap_or("missing or non-string")
            ),
            Self::MissingRule => formatter.write_str("policy has no `rule` field"),
            Self::InvalidFieldType {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "policy field `{field}` must be {expected}, found {actual}"
            ),
            Self::UnsupportedCandidateType { index, actual } => write!(
                formatter,
                "policy candidate {index} must be a named-rule string, found {actual}"
            ),
            Self::UnsafeThreshold { actual } => write!(
                formatter,
                "policy must use k-of-n = 1, found {}",
                actual.map_or_else(|| "missing or out of range".to_owned(), |v| v.to_string())
            ),
            Self::MissingPasswordFallback => {
                formatter.write_str("policy is missing `use-login-window-ui`")
            }
            Self::DuplicatePasswordFallbacks { count } => write!(
                formatter,
                "policy has {count} `use-login-window-ui` candidates; expected one"
            ),
            Self::MissingReposeCandidate => {
                formatter.write_str("Repose candidate is not installed")
            }
            Self::DuplicateReposeCandidates { count } => {
                write!(
                    formatter,
                    "policy has {count} Repose candidates; expected at most one"
                )
            }
            Self::MisplacedReposeCandidate {
                repose_index,
                fallback_index,
            } => write!(
                formatter,
                "Repose candidate at {repose_index} must immediately precede fallback at {fallback_index}"
            ),
            Self::Serialization { detail } => {
                write!(formatter, "failed to serialize policy plist: {detail}")
            }
        }
    }
}

impl std::error::Error for PolicyError {}

fn detect_encoding(input: &[u8]) -> Result<(Encoding, &[u8]), PolicyError> {
    if input.is_empty() {
        return Err(PolicyError::UnsupportedEncoding);
    }
    if input.starts_with(b"bplist00") {
        return Ok((Encoding::Binary, input));
    }

    let mut payload = input;
    if payload.starts_with(&[0xEF, 0xBB, 0xBF]) {
        payload = &payload[3..];
    }
    let first_content = payload
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(payload.len());
    payload = &payload[first_content..];
    if payload.starts_with(b"<") {
        Ok((Encoding::Xml, payload))
    } else {
        Err(PolicyError::UnsupportedEncoding)
    }
}

#[derive(Debug)]
enum CollectionFrame {
    Array,
    Dictionary {
        expecting_key: bool,
        keys: HashSet<String>,
    },
}

fn preflight_event_stream(
    input: &[u8],
    maximum_depth: usize,
    maximum_events: usize,
    maximum_expanded_bytes: usize,
) -> Result<(), PolicyError> {
    let mut stack: Vec<CollectionFrame> = Vec::new();
    let mut root_values = 0usize;
    let mut event_count = 0usize;
    let mut expanded_bytes = 0usize;

    for event in Reader::new(Cursor::new(input)) {
        let event = event.map_err(malformed)?;
        event_count =
            event_count
                .checked_add(1)
                .ok_or(PolicyError::ExpandedEventLimitExceeded {
                    maximum: maximum_events,
                })?;
        if event_count > maximum_events {
            return Err(PolicyError::ExpandedEventLimitExceeded {
                maximum: maximum_events,
            });
        }

        expanded_bytes = expanded_bytes
            .checked_add(event_expanded_bytes(&event))
            .ok_or(PolicyError::ExpandedByteLimitExceeded {
                maximum: maximum_expanded_bytes,
            })?;
        if expanded_bytes > maximum_expanded_bytes {
            return Err(PolicyError::ExpandedByteLimitExceeded {
                maximum: maximum_expanded_bytes,
            });
        }

        match event {
            Event::StartArray(_) => {
                ensure_collection_can_start(&stack)?;
                stack.push(CollectionFrame::Array);
                if stack.len() > maximum_depth {
                    return Err(PolicyError::NestingTooDeep {
                        maximum: maximum_depth,
                    });
                }
            }
            Event::StartDictionary(_) => {
                ensure_collection_can_start(&stack)?;
                stack.push(CollectionFrame::Dictionary {
                    expecting_key: true,
                    keys: HashSet::new(),
                });
                if stack.len() > maximum_depth {
                    return Err(PolicyError::NestingTooDeep {
                        maximum: maximum_depth,
                    });
                }
            }
            Event::EndCollection => {
                let frame = stack
                    .pop()
                    .ok_or_else(|| malformed_text("unexpected collection end"))?;
                if matches!(
                    frame,
                    CollectionFrame::Dictionary {
                        expecting_key: false,
                        ..
                    }
                ) {
                    return Err(malformed_text("dictionary key has no value"));
                }
                complete_value(&mut stack, &mut root_values)?;
            }
            Event::String(value)
                if matches!(
                    stack.last(),
                    Some(CollectionFrame::Dictionary {
                        expecting_key: true,
                        ..
                    })
                ) =>
            {
                let Some(CollectionFrame::Dictionary {
                    expecting_key,
                    keys,
                }) = stack.last_mut()
                else {
                    return Err(malformed_text(
                        "dictionary key appeared outside a dictionary",
                    ));
                };
                let key = value.into_owned();
                if !keys.insert(key.clone()) {
                    return Err(PolicyError::DuplicateDictionaryKey { key });
                }
                *expecting_key = false;
            }
            Event::Boolean(_)
            | Event::Data(_)
            | Event::Date(_)
            | Event::Integer(_)
            | Event::Real(_)
            | Event::String(_)
            | Event::Uid(_) => complete_value(&mut stack, &mut root_values)?,
            _ => return Err(malformed_text("unsupported plist event")),
        }
    }

    if !stack.is_empty() || root_values != 1 {
        return Err(malformed_text(
            "property list must contain exactly one root value",
        ));
    }
    Ok(())
}

fn event_expanded_bytes(event: &Event<'_>) -> usize {
    match event {
        Event::String(value) => value.len(),
        Event::Data(value) => value.len(),
        Event::Boolean(_) => 1,
        Event::Date(_) | Event::Real(_) | Event::Uid(_) => 8,
        Event::Integer(_) => 16,
        Event::StartArray(_) | Event::StartDictionary(_) | Event::EndCollection => 0,
        _ => 0,
    }
}

fn ensure_collection_can_start(stack: &[CollectionFrame]) -> Result<(), PolicyError> {
    if matches!(
        stack.last(),
        Some(CollectionFrame::Dictionary {
            expecting_key: true,
            ..
        })
    ) {
        Err(malformed_text("dictionary keys must be strings"))
    } else {
        Ok(())
    }
}

fn complete_value(
    stack: &mut [CollectionFrame],
    root_values: &mut usize,
) -> Result<(), PolicyError> {
    match stack.last_mut() {
        Some(CollectionFrame::Array) => Ok(()),
        Some(CollectionFrame::Dictionary { expecting_key, .. }) if !*expecting_key => {
            *expecting_key = true;
            Ok(())
        }
        Some(CollectionFrame::Dictionary { .. }) => Err(malformed_text(
            "dictionary value appeared where a key was required",
        )),
        None => {
            *root_values += 1;
            if *root_values > 1 {
                Err(malformed_text("property list has multiple root values"))
            } else {
                Ok(())
            }
        }
    }
}

fn malformed(error: plist::Error) -> PolicyError {
    PolicyError::MalformedPlist {
        detail: error.to_string(),
    }
}

fn malformed_text(detail: &str) -> PolicyError {
    PolicyError::MalformedPlist {
        detail: detail.to_owned(),
    }
}

fn serialization(error: plist::Error) -> PolicyError {
    PolicyError::Serialization {
        detail: error.to_string(),
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Array(_) => "array",
        Value::Dictionary(_) => "dictionary",
        Value::Boolean(_) => "boolean",
        Value::Data(_) => "data",
        Value::Date(_) => "date",
        Value::Real(_) => "real",
        Value::Integer(_) => "integer",
        Value::String(_) => "string",
        Value::Uid(_) => "uid",
        _ => "unknown",
    }
}
