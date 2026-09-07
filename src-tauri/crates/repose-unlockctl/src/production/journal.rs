use super::*;

impl BackupRecord {
    pub(super) fn new(policy: &[u8]) -> Self {
        Self {
            policy: policy.to_vec(),
        }
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, BackendError> {
        if self.policy.len() > MAX_POLICY_BYTES {
            return Err(BackendError::new("policy backup is too large"));
        }
        let mut root = Dictionary::new();
        root.insert(
            "format".to_owned(),
            Value::String("repose-unlockctl-backup-v1".to_owned()),
        );
        root.insert("policy".to_owned(), Value::Data(self.policy.clone()));
        root.insert(
            "sha256".to_owned(),
            Value::String(crate::artifact_verify::sha256_hex(&self.policy)),
        );
        let mut bytes = Vec::new();
        Value::Dictionary(root)
            .to_writer_binary(&mut bytes)
            .map_err(|error| BackendError::new(error.to_string()))?;
        Ok(bytes)
    }

    pub(super) fn decode(bytes: &[u8]) -> Result<Self, BackendError> {
        if bytes.len() > MAX_POLICY_BYTES + 4096 {
            return Err(BackendError::new("backup container is too large"));
        }
        let value = Value::from_reader(std::io::Cursor::new(bytes))
            .map_err(|error| BackendError::new(error.to_string()))?;
        let root = value
            .as_dictionary()
            .ok_or_else(|| BackendError::new("backup container is not a dictionary"))?;
        if root.len() != 3
            || root.get("format").and_then(Value::as_string) != Some("repose-unlockctl-backup-v1")
        {
            return Err(BackendError::new("backup container schema is invalid"));
        }
        let policy = root
            .get("policy")
            .and_then(Value::as_data)
            .ok_or_else(|| BackendError::new("backup policy is missing"))?
            .to_vec();
        if policy.len() > MAX_POLICY_BYTES {
            return Err(BackendError::new("policy backup is too large"));
        }
        let digest = root
            .get("sha256")
            .and_then(Value::as_string)
            .ok_or_else(|| BackendError::new("backup checksum is missing"))?;
        if digest != crate::artifact_verify::sha256_hex(&policy) {
            return Err(BackendError::new("backup checksum mismatch"));
        }
        Ok(Self { policy })
    }
}

#[derive(Clone)]
pub(super) struct JournalRecord {
    pub(super) operation: JournalOperation,
    pub(super) phase: JournalPhase,
    pub(super) policy: Vec<u8>,
    pub(super) named_rule: Option<Vec<u8>>,
    pub(super) repair_base: Option<Vec<u8>>,
    pub(super) prior_receipt: InstallReceiptState,
    pub(super) target_receipt: Option<InstallReceiptFingerprint>,
    pub(super) identities: [Option<FileIdentity>; 4],
}

impl JournalRecord {
    pub(super) fn durable_view(&self) -> DurableJournal {
        DurableJournal {
            operation: self.operation,
            phase: self.phase,
            policy: self.policy.clone(),
            named_rule: self.named_rule.clone(),
            repair_base: self.repair_base.clone(),
            prior_receipt: self.prior_receipt,
            target_receipt: self.target_receipt,
        }
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, BackendError> {
        let mut root = Dictionary::new();
        root.insert(
            "format".to_owned(),
            Value::String("repose-unlockctl-journal-v1".to_owned()),
        );
        root.insert(
            "operation".to_owned(),
            Value::String(operation_name(self.operation).to_owned()),
        );
        root.insert(
            "phase".to_owned(),
            Value::String(phase_name(self.phase).to_owned()),
        );
        root.insert("policy".to_owned(), Value::Data(self.policy.clone()));
        if let Some(named) = &self.named_rule {
            root.insert("named".to_owned(), Value::Data(named.clone()));
        }
        if let Some(repair_base) = &self.repair_base {
            root.insert("repair-base".to_owned(), Value::Data(repair_base.clone()));
        }
        if let InstallReceiptState::Trusted(fingerprint) = self.prior_receipt {
            root.insert(
                "prior-receipt".to_owned(),
                Value::Data(fingerprint.bytes().to_vec()),
            );
        }
        if let Some(fingerprint) = self.target_receipt {
            root.insert(
                "target-receipt".to_owned(),
                Value::Data(fingerprint.bytes().to_vec()),
            );
        }
        root.insert(
            "identities".to_owned(),
            Value::Array(
                self.identities
                    .iter()
                    .map(|identity| match identity {
                        Some(identity) => {
                            Value::String(format!("{}:{}", identity.device, identity.inode))
                        }
                        None => Value::String("missing".to_owned()),
                    })
                    .collect(),
            ),
        );
        let mut bytes = Vec::new();
        Value::Dictionary(root)
            .to_writer_binary(&mut bytes)
            .map_err(|error| BackendError::new(error.to_string()))?;
        Ok(bytes)
    }

    pub(super) fn decode(bytes: &[u8]) -> Result<Self, BackendError> {
        if bytes.len() > MAX_JOURNAL_BYTES {
            return Err(BackendError::new("transaction journal is too large"));
        }
        let value = Value::from_reader(std::io::Cursor::new(bytes))
            .map_err(|error| BackendError::new(error.to_string()))?;
        let root = value
            .as_dictionary()
            .ok_or_else(|| BackendError::new("transaction journal is not a dictionary"))?;
        let allowed: BTreeMap<&str, bool> = [
            ("format", true),
            ("operation", true),
            ("phase", true),
            ("policy", true),
            ("named", false),
            ("repair-base", false),
            ("prior-receipt", false),
            ("target-receipt", false),
            ("identities", true),
        ]
        .into_iter()
        .collect();
        if root.keys().any(|key| !allowed.contains_key(key.as_str()))
            || allowed
                .iter()
                .any(|(key, required)| *required && !root.contains_key(key))
        {
            return Err(BackendError::new("transaction journal keys are invalid"));
        }
        if root.get("format").and_then(Value::as_string) != Some("repose-unlockctl-journal-v1") {
            return Err(BackendError::new("transaction journal format is invalid"));
        }
        let policy = root
            .get("policy")
            .and_then(Value::as_data)
            .ok_or_else(|| BackendError::new("journal policy is missing"))?
            .to_vec();
        if policy.len() > MAX_POLICY_BYTES {
            return Err(BackendError::new("journal policy is too large"));
        }
        let named_rule = root
            .get("named")
            .map(|value| {
                value
                    .as_data()
                    .map(<[u8]>::to_vec)
                    .ok_or_else(|| BackendError::new("journal named rule is invalid"))
            })
            .transpose()?;
        let repair_base = root
            .get("repair-base")
            .map(|value| {
                value
                    .as_data()
                    .map(<[u8]>::to_vec)
                    .ok_or_else(|| BackendError::new("journal repair base is invalid"))
            })
            .transpose()?;
        if repair_base
            .as_ref()
            .is_some_and(|bytes| bytes.len() > MAX_POLICY_BYTES)
        {
            return Err(BackendError::new("journal repair base is too large"));
        }
        let identities = decode_identities(
            root.get("identities")
                .and_then(Value::as_array)
                .ok_or_else(|| BackendError::new("journal identities are missing"))?,
        )?;
        let prior_receipt = match root.get("prior-receipt") {
            None => InstallReceiptState::Missing,
            Some(value) => {
                let bytes = value
                    .as_data()
                    .ok_or_else(|| BackendError::new("journal receipt fingerprint is invalid"))?;
                let fingerprint: [u8; 32] = bytes.try_into().map_err(|_| {
                    BackendError::new("journal receipt fingerprint length is invalid")
                })?;
                InstallReceiptState::Trusted(InstallReceiptFingerprint::new(fingerprint))
            }
        };
        let target_receipt = root
            .get("target-receipt")
            .map(|value| {
                let bytes = value.as_data().ok_or_else(|| {
                    BackendError::new("journal target receipt fingerprint is invalid")
                })?;
                let fingerprint: [u8; 32] = bytes.try_into().map_err(|_| {
                    BackendError::new("journal target receipt fingerprint length is invalid")
                })?;
                Ok(InstallReceiptFingerprint::new(fingerprint))
            })
            .transpose()?;
        Ok(Self {
            operation: parse_operation(
                root.get("operation")
                    .and_then(Value::as_string)
                    .ok_or_else(|| BackendError::new("journal operation is invalid"))?,
            )?,
            phase: parse_phase(
                root.get("phase")
                    .and_then(Value::as_string)
                    .ok_or_else(|| BackendError::new("journal phase is invalid"))?,
            )?,
            policy,
            named_rule,
            repair_base,
            prior_receipt,
            target_receipt,
            identities,
        })
    }
}

pub(super) fn decode_identities(
    values: &[Value],
) -> Result<[Option<FileIdentity>; 4], BackendError> {
    if values.len() != 4 {
        return Err(BackendError::new("journal identity count is invalid"));
    }
    let mut result = [None; 4];
    for (index, value) in values.iter().enumerate() {
        let value = value
            .as_string()
            .ok_or_else(|| BackendError::new("journal identity is invalid"))?;
        if value == "missing" {
            continue;
        }
        let (device, inode) = value
            .split_once(':')
            .ok_or_else(|| BackendError::new("journal identity is malformed"))?;
        result[index] = Some(FileIdentity {
            device: device
                .parse()
                .map_err(|_| BackendError::new("journal device is invalid"))?,
            inode: inode
                .parse()
                .map_err(|_| BackendError::new("journal inode is invalid"))?,
        });
    }
    Ok(result)
}

pub(super) fn operation_name(operation: JournalOperation) -> &'static str {
    match operation {
        JournalOperation::Install => "install",
        JournalOperation::Uninstall => "uninstall",
        JournalOperation::Repair => "repair",
    }
}

pub(super) fn parse_operation(value: &str) -> Result<JournalOperation, BackendError> {
    match value {
        "install" => Ok(JournalOperation::Install),
        "uninstall" => Ok(JournalOperation::Uninstall),
        "repair" => Ok(JournalOperation::Repair),
        _ => Err(BackendError::new("journal operation is unknown")),
    }
}

pub(super) fn phase_name(phase: JournalPhase) -> &'static str {
    match phase {
        JournalPhase::Prepared => "prepared",
        JournalPhase::PolicyInactive => "policy-inactive",
        JournalPhase::ComponentsCommittedHealthy => "components-healthy",
        JournalPhase::NamedReady => "named-ready",
        JournalPhase::PolicyActive => "policy-active",
        JournalPhase::Committed => "committed",
        JournalPhase::ComponentsRemoved => "components-removed",
        JournalPhase::Aborted => "aborted",
    }
}

pub(super) fn parse_phase(value: &str) -> Result<JournalPhase, BackendError> {
    match value {
        "prepared" => Ok(JournalPhase::Prepared),
        "policy-inactive" => Ok(JournalPhase::PolicyInactive),
        "components-healthy" => Ok(JournalPhase::ComponentsCommittedHealthy),
        "named-ready" => Ok(JournalPhase::NamedReady),
        "policy-active" => Ok(JournalPhase::PolicyActive),
        "committed" => Ok(JournalPhase::Committed),
        "components-removed" => Ok(JournalPhase::ComponentsRemoved),
        "aborted" => Ok(JournalPhase::Aborted),
        _ => Err(BackendError::new("journal phase is unknown")),
    }
}
