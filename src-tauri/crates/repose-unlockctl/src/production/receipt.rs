use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FileIdentity {
    pub(super) device: u64,
    pub(super) inode: u64,
}

pub(super) struct BackupRecord {
    pub(super) policy: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ComponentReceipt {
    pub(super) identity: FileIdentity,
    pub(super) kind: String,
    pub(super) uid: u32,
    pub(super) gid: u32,
    pub(super) mode: u32,
    pub(super) links: u64,
    pub(super) flags: u32,
    pub(super) digest: String,
    pub(super) signing_requirement: String,
    pub(super) security_policy: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct InstallReceipt {
    pub(super) generation: String,
    pub(super) package_version: String,
    pub(super) minimum_os: String,
    pub(super) architecture: String,
    pub(super) team_id: String,
    pub(super) components: [ComponentReceipt; 4],
}

impl InstallReceipt {
    pub(super) const FORMAT: &'static str = "repose-unlockctl-install-receipt-v1";
    pub(super) const PROTOCOL_VERSION: &'static str = "1";
    pub(super) const BUILD_KIND: &'static str = "deny-only";
    pub(super) const SECURITY_POLICY: &'static str = "no-acl-no-xattr-no-flags-v1";

    pub(super) fn decode(bytes: &[u8]) -> Result<Self, BackendError> {
        if bytes.len() > MAX_RECEIPT_BYTES {
            return Err(BackendError::new("install receipt is too large"));
        }
        let value = Value::from_reader(std::io::Cursor::new(bytes))
            .map_err(|error| BackendError::new(error.to_string()))?;
        let root = value
            .as_dictionary()
            .ok_or_else(|| BackendError::new("install receipt is not a dictionary"))?;
        let keys = [
            "format",
            "generation",
            "package-version",
            "protocol-version",
            "minimum-os",
            "architecture",
            "deny-only-build-kind",
            "team-id",
            "components",
        ];
        if root.len() != keys.len() || keys.iter().any(|key| !root.contains_key(key)) {
            return Err(BackendError::new("install receipt keys are invalid"));
        }
        if required_string(root, "format")? != Self::FORMAT
            || required_string(root, "protocol-version")? != Self::PROTOCOL_VERSION
            || required_string(root, "deny-only-build-kind")? != Self::BUILD_KIND
        {
            return Err(BackendError::new(
                "install receipt security profile is invalid",
            ));
        }
        let components = root
            .get("components")
            .and_then(Value::as_array)
            .ok_or_else(|| BackendError::new("install receipt components are missing"))?;
        if components.len() != Component::INSTALL_ORDER.len() {
            return Err(BackendError::new(
                "install receipt component count is invalid",
            ));
        }
        let mut decoded = Vec::with_capacity(components.len());
        for (index, value) in components.iter().enumerate() {
            decoded.push(ComponentReceipt::decode(
                value,
                Component::INSTALL_ORDER[index],
            )?);
        }
        let components: [ComponentReceipt; 4] = decoded
            .try_into()
            .map_err(|_| BackendError::new("install receipt component count is invalid"))?;
        let receipt = Self {
            generation: bounded_receipt_string(root, "generation")?,
            package_version: bounded_receipt_string(root, "package-version")?,
            minimum_os: bounded_receipt_string(root, "minimum-os")?,
            architecture: bounded_receipt_string(root, "architecture")?,
            team_id: bounded_receipt_string(root, "team-id")?,
            components,
        };
        if receipt.generation.is_empty()
            || receipt.package_version.is_empty()
            || receipt.minimum_os.is_empty()
            || receipt.architecture.is_empty()
            || receipt.team_id.is_empty()
        {
            return Err(BackendError::new("install receipt identity is empty"));
        }
        Ok(receipt)
    }

    pub(super) fn identities(&self) -> [Option<FileIdentity>; 4] {
        self.components
            .each_ref()
            .map(|component| Some(component.identity))
    }

    #[cfg(test)]
    pub(super) fn encode(&self) -> Result<Vec<u8>, BackendError> {
        let mut root = Dictionary::new();
        for (key, value) in [
            ("format", Self::FORMAT),
            ("generation", self.generation.as_str()),
            ("package-version", self.package_version.as_str()),
            ("protocol-version", Self::PROTOCOL_VERSION),
            ("minimum-os", self.minimum_os.as_str()),
            ("architecture", self.architecture.as_str()),
            ("deny-only-build-kind", Self::BUILD_KIND),
            ("team-id", self.team_id.as_str()),
        ] {
            root.insert(key.to_owned(), Value::String(value.to_owned()));
        }
        root.insert(
            "components".to_owned(),
            Value::Array(
                self.components
                    .iter()
                    .zip(Component::INSTALL_ORDER)
                    .map(|(receipt, component)| receipt.to_value(component))
                    .collect(),
            ),
        );
        let mut bytes = Vec::new();
        Value::Dictionary(root)
            .to_writer_binary(&mut bytes)
            .map_err(|error| BackendError::new(error.to_string()))?;
        if bytes.len() > MAX_RECEIPT_BYTES {
            return Err(BackendError::new("install receipt is too large"));
        }
        Ok(bytes)
    }
}

impl ComponentReceipt {
    pub(super) fn decode(value: &Value, component: Component) -> Result<Self, BackendError> {
        let dictionary = value
            .as_dictionary()
            .ok_or_else(|| BackendError::new("component receipt is not a dictionary"))?;
        let keys = [
            "name",
            "device",
            "inode",
            "kind",
            "uid",
            "gid",
            "mode",
            "links",
            "flags",
            "sha256",
            "signing-requirement",
            "security-policy",
        ];
        if dictionary.len() != keys.len()
            || keys.iter().any(|key| !dictionary.contains_key(key))
            || required_string(dictionary, "name")? != component_name(component)
        {
            return Err(BackendError::new("component receipt schema is invalid"));
        }
        let digest = required_string(dictionary, "sha256")?.to_owned();
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(BackendError::new("component receipt digest is invalid"));
        }
        let security_policy = required_string(dictionary, "security-policy")?.to_owned();
        if security_policy != InstallReceipt::SECURITY_POLICY {
            return Err(BackendError::new(
                "component receipt filesystem policy is invalid",
            ));
        }
        let signing_requirement = bounded_receipt_string(dictionary, "signing-requirement")?;
        if signing_requirement.is_empty() {
            return Err(BackendError::new(
                "component receipt signing requirement is empty",
            ));
        }
        let receipt = Self {
            identity: FileIdentity {
                device: receipt_u64(dictionary, "device")?,
                inode: receipt_u64(dictionary, "inode")?,
            },
            kind: required_string(dictionary, "kind")?.to_owned(),
            uid: receipt_u32(dictionary, "uid")?,
            gid: receipt_u32(dictionary, "gid")?,
            mode: receipt_u32(dictionary, "mode")?,
            links: receipt_u64(dictionary, "links")?,
            flags: receipt_u32(dictionary, "flags")?,
            digest,
            signing_requirement,
            security_policy,
        };
        if receipt.kind != component_kind(component)
            || receipt.mode != component_mode(component)
            || receipt.flags != 0
        {
            return Err(BackendError::new("component receipt metadata is invalid"));
        }
        Ok(receipt)
    }

    #[cfg(test)]
    pub(super) fn to_value(&self, component: Component) -> Value {
        let mut dictionary = Dictionary::new();
        for (key, value) in [
            ("name", component_name(component).to_owned()),
            ("device", self.identity.device.to_string()),
            ("inode", self.identity.inode.to_string()),
            ("kind", self.kind.clone()),
            ("uid", self.uid.to_string()),
            ("gid", self.gid.to_string()),
            ("mode", self.mode.to_string()),
            ("links", self.links.to_string()),
            ("flags", self.flags.to_string()),
            ("sha256", self.digest.clone()),
            ("signing-requirement", self.signing_requirement.clone()),
            ("security-policy", self.security_policy.clone()),
        ] {
            dictionary.insert(key.to_owned(), Value::String(value));
        }
        Value::Dictionary(dictionary)
    }
}

pub(super) fn required_string<'a>(
    dictionary: &'a Dictionary,
    key: &str,
) -> Result<&'a str, BackendError> {
    dictionary
        .get(key)
        .and_then(Value::as_string)
        .ok_or_else(|| BackendError::new(format!("receipt field is invalid: {key}")))
}

pub(super) fn bounded_receipt_string(
    dictionary: &Dictionary,
    key: &str,
) -> Result<String, BackendError> {
    let value = required_string(dictionary, key)?;
    if value.len() > 1024 {
        return Err(BackendError::new(format!(
            "receipt field is too large: {key}"
        )));
    }
    Ok(value.to_owned())
}

pub(super) fn receipt_u64(dictionary: &Dictionary, key: &str) -> Result<u64, BackendError> {
    required_string(dictionary, key)?
        .parse()
        .map_err(|_| BackendError::new(format!("receipt integer is invalid: {key}")))
}

pub(super) fn receipt_u32(dictionary: &Dictionary, key: &str) -> Result<u32, BackendError> {
    required_string(dictionary, key)?
        .parse()
        .map_err(|_| BackendError::new(format!("receipt integer is invalid: {key}")))
}
