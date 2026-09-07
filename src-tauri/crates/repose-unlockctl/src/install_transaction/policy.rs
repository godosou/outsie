use super::*;
use std::collections::HashSet;
use std::io::Cursor;

use plist::{
    Dictionary, Value,
    stream::{Event, Reader},
};

#[must_use]
pub fn named_rule_v1() -> Vec<u8> {
    let mut dictionary = Dictionary::new();
    dictionary.insert(
        "class".to_owned(),
        Value::String("evaluate-mechanisms".to_owned()),
    );
    dictionary.insert(
        "mechanisms".to_owned(),
        Value::Array(vec![Value::String(PLUGIN_MECHANISM.to_owned())]),
    );
    let mut bytes = Vec::new();
    Value::Dictionary(dictionary)
        .to_writer_xml(&mut bytes)
        .expect("serializing a fixed in-memory plist cannot fail");
    bytes
}

pub fn verify_named_rule_v1(bytes: &[u8]) -> Result<(), PolicyError> {
    preflight_named_rule(bytes)?;
    let actual = Value::from_reader(std::io::Cursor::new(bytes)).map_err(|error| {
        PolicyError::MalformedPlist {
            detail: error.to_string(),
        }
    })?;
    let expected = Value::from_reader(std::io::Cursor::new(named_rule_v1())).map_err(|error| {
        PolicyError::MalformedPlist {
            detail: error.to_string(),
        }
    })?;
    if actual != expected {
        return Err(PolicyError::MalformedPlist {
            detail: "named rule must contain only the fixed evaluate-mechanisms definition"
                .to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug)]
enum NamedCollection {
    Array,
    Dictionary {
        expecting_key: bool,
        keys: HashSet<String>,
    },
}

fn preflight_named_rule(bytes: &[u8]) -> Result<(), PolicyError> {
    if bytes.len() > ScreenSaverPolicy::MAX_INPUT_BYTES {
        return Err(PolicyError::InputTooLarge {
            actual: bytes.len(),
            maximum: ScreenSaverPolicy::MAX_INPUT_BYTES,
        });
    }
    let mut stack = Vec::new();
    let mut roots = 0usize;
    let mut events = 0usize;
    for event in Reader::new(Cursor::new(bytes)) {
        let event = event.map_err(|error| PolicyError::MalformedPlist {
            detail: error.to_string(),
        })?;
        events += 1;
        if events > 128 || stack.len() > 16 {
            return Err(PolicyError::MalformedPlist {
                detail: "named rule exceeds structural limits".to_owned(),
            });
        }
        match event {
            Event::StartArray(_) => {
                named_collection_can_start(&stack)?;
                stack.push(NamedCollection::Array);
            }
            Event::StartDictionary(_) => {
                named_collection_can_start(&stack)?;
                stack.push(NamedCollection::Dictionary {
                    expecting_key: true,
                    keys: HashSet::new(),
                });
            }
            Event::EndCollection => {
                let frame = stack.pop().ok_or_else(named_malformed)?;
                if matches!(
                    frame,
                    NamedCollection::Dictionary {
                        expecting_key: false,
                        ..
                    }
                ) {
                    return Err(named_malformed());
                }
                complete_named_value(&mut stack, &mut roots)?;
            }
            Event::String(value)
                if matches!(
                    stack.last(),
                    Some(NamedCollection::Dictionary {
                        expecting_key: true,
                        ..
                    })
                ) =>
            {
                let Some(NamedCollection::Dictionary {
                    expecting_key,
                    keys,
                }) = stack.last_mut()
                else {
                    return Err(named_malformed());
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
            | Event::Uid(_) => complete_named_value(&mut stack, &mut roots)?,
            _ => return Err(named_malformed()),
        }
    }
    if !stack.is_empty() || roots != 1 {
        return Err(named_malformed());
    }
    Ok(())
}

fn named_collection_can_start(stack: &[NamedCollection]) -> Result<(), PolicyError> {
    if matches!(
        stack.last(),
        Some(NamedCollection::Dictionary {
            expecting_key: true,
            ..
        })
    ) {
        Err(named_malformed())
    } else {
        Ok(())
    }
}

fn complete_named_value(
    stack: &mut [NamedCollection],
    roots: &mut usize,
) -> Result<(), PolicyError> {
    match stack.last_mut() {
        Some(NamedCollection::Array) => Ok(()),
        Some(NamedCollection::Dictionary { expecting_key, .. }) if !*expecting_key => {
            *expecting_key = true;
            Ok(())
        }
        Some(NamedCollection::Dictionary { .. }) => Err(named_malformed()),
        None => {
            *roots += 1;
            if *roots == 1 {
                Ok(())
            } else {
                Err(named_malformed())
            }
        }
    }
}

fn named_malformed() -> PolicyError {
    PolicyError::MalformedPlist {
        detail: "named rule has an invalid event structure".to_owned(),
    }
}

pub(super) fn verify_repose_absent(bytes: &[u8]) -> Result<(), BackendError> {
    let parsed =
        ScreenSaverPolicy::parse(bytes).map_err(|error| BackendError::new(error.to_string()))?;
    if parsed.repose_candidate_index().is_some() {
        return Err(BackendError::new(
            "Repose candidate remains in screensaver rule",
        ));
    }
    if !parsed.has_exactly_one_password_fallback() {
        return Err(BackendError::new("password fallback is not unique"));
    }
    Ok(())
}

pub(super) fn policies_equivalent(left: &[u8], right: &[u8]) -> Result<bool, BackendError> {
    let left =
        ScreenSaverPolicy::parse(left).map_err(|error| BackendError::new(error.to_string()))?;
    let right =
        ScreenSaverPolicy::parse(right).map_err(|error| BackendError::new(error.to_string()))?;
    Ok(left.structurally_equivalent(&right))
}

pub(super) fn same_named_definition(left: Option<&[u8]>, right: Option<&[u8]>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            verify_named_rule_v1(left).is_ok() && verify_named_rule_v1(right).is_ok()
        }
        _ => false,
    }
}

pub(super) fn surgically_remove_live_repose<B: InstallBackend>(
    backend: &mut B,
    expected: Option<&[u8]>,
) -> Result<(), BackendError> {
    let live = backend.read_screensaver_rule()?;
    let parsed =
        ScreenSaverPolicy::parse(&live).map_err(|error| BackendError::new(error.to_string()))?;
    if parsed.repose_candidate_index().is_none() {
        verify_repose_absent(&live)?;
        return Ok(());
    }
    if let Some(expected) = expected {
        // Exact equality is the only case where a preimage could be safe, but
        // even then surgical removal better preserves unrelated live keys.
        let _exact_transaction_value = live == expected;
    }
    let removed = parsed
        .remove(&PolicySpec::v1())
        .map_err(|error| BackendError::new(error.to_string()))?
        .to_bytes()
        .map_err(|error| BackendError::new(error.to_string()))?;
    backend.set_screensaver_rule(&removed)?;
    let readback = backend.read_screensaver_rule()?;
    verify_repose_absent(&readback)?;
    if !policies_equivalent(&readback, &removed)? {
        return Err(BackendError::new(
            "surgical screensaver readback differs from the latest-live transform",
        ));
    }
    Ok(())
}

pub(super) fn deactivate_policy_for_repair<B: InstallBackend>(
    backend: &mut B,
    validated_fallback: &ScreenSaverPolicy,
) -> Result<(), BackendError> {
    let live = backend.read_screensaver_rule()?;
    let base = ScreenSaverPolicy::parse(&live).unwrap_or_else(|_| validated_fallback.clone());
    let inactive = base
        .remove(&PolicySpec::v1())
        .or_else(|_| validated_fallback.clone().remove(&PolicySpec::v1()))
        .and_then(|policy| policy.to_bytes())
        .map_err(|error| BackendError::new(error.to_string()))?;
    let already_inactive = verify_repose_absent(&live).is_ok()
        && policies_equivalent(&live, &inactive).unwrap_or(false);
    if !already_inactive {
        backend.set_screensaver_rule(&inactive)?;
    }
    let readback = backend.read_screensaver_rule()?;
    verify_repose_absent(&readback)?;
    if !policies_equivalent(&readback, &inactive)? {
        return Err(BackendError::new(
            "repair deactivation readback differs from password-only target",
        ));
    }
    Ok(())
}

pub(super) fn restore_named_if_unchanged<B: InstallBackend>(
    backend: &mut B,
    previous: Option<&[u8]>,
) -> Result<(), BackendError> {
    let current = backend.read_named_rule()?;
    if current
        .as_deref()
        .is_none_or(|bytes| verify_named_rule_v1(bytes).is_err())
    {
        return Err(BackendError::new(
            "live named rule changed concurrently; left untouched",
        ));
    }
    match previous {
        Some(bytes) => backend.set_named_rule(bytes)?,
        None => backend.remove_named_rule()?,
    }
    let readback = backend.read_named_rule()?;
    match (readback.as_deref(), previous) {
        (None, None) => {}
        (Some(actual), Some(expected))
            if verify_named_rule_v1(actual).is_ok() && verify_named_rule_v1(expected).is_ok() => {}
        _ => return Err(BackendError::new("named-rule rollback readback differs")),
    }
    Ok(())
}

pub(super) fn restore_repose_on_latest_live_policy<B: InstallBackend>(
    backend: &mut B,
    expected_receipt: InstallReceiptState,
) -> Result<(), BackendError> {
    let live = backend.read_screensaver_rule()?;
    let restored = ScreenSaverPolicy::parse(&live)
        .map_err(|error| BackendError::new(error.to_string()))?
        .install(&PolicySpec::v1())
        .map_err(|error| BackendError::new(error.to_string()))?
        .to_bytes()
        .map_err(|error| BackendError::new(error.to_string()))?;
    let activation = (|| {
        backend.set_screensaver_rule(&restored)?;
        let readback = backend.read_screensaver_rule()?;
        ScreenSaverPolicy::parse(&readback)
            .and_then(|policy| policy.verify_installed(&PolicySpec::v1()))
            .map_err(|error| BackendError::new(error.to_string()))?;
        if !policies_equivalent(&readback, &restored)? {
            return Err(BackendError::new(
                "restored screensaver readback differs from latest-live transform",
            ));
        }
        if !active_dependency_closure(backend, expected_receipt)? {
            return Err(BackendError::new(
                "restored authorization dependency closure is not exact",
            ));
        }
        Ok(())
    })();
    if let Err(primary) = activation {
        return match surgically_remove_live_repose(backend, Some(&restored)) {
            Ok(()) => Err(primary),
            Err(deactivation) => Err(BackendError::new(format!(
                "{primary}; could not prove failed activation inactive: {deactivation}"
            ))),
        };
    }
    Ok(())
}

pub(super) fn live_policy_is_inactive<B: InstallBackend>(
    backend: &mut B,
) -> Result<bool, BackendError> {
    let live = backend.read_screensaver_rule()?;
    Ok(verify_repose_absent(&live).is_ok())
}
