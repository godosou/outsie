//! Read-only inspection for the fixed macOS authorization prototype package.
//!
//! The package checksum file detects accidental damage; it is not a trust
//! anchor and this module deliberately returns a plan-only type. The launchd
//! property list is compared byte-for-byte with an installer-embedded template
//! so a rewritten checksum cannot bless a different root command. Production
//! apply returns a distinct, unconstructible sealed capability and remains
//! closed until Task 8 implements descriptor-stable staging and attestation.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component as PathComponent, Path, PathBuf};

use sha2::{Digest, Sha256};

pub const PLUGIN_RELATIVE_PATH: &str = "ReposeUnlock.bundle";
pub const PLUGIN_EXECUTABLE_RELATIVE_PATH: &str = "ReposeUnlock.bundle/Contents/MacOS/ReposeUnlock";
pub const SERVICE_RELATIVE_PATH: &str = "bin/ai.repose.unlockd";
pub const LAUNCHD_RELATIVE_PATH: &str = "launchd/ai.repose.unlockd.plist";
pub const MANIFEST_NAME: &str = "SHA256SUMS";
const MANIFEST_VERSION: &str = "repose-package-v1";
const DEVELOPMENT_MODE: &str = "mode=adhoc-development";
const SIGNED_MODE: &str = "mode=developer-id";
const MAX_MANIFEST_BYTES: u64 = 32 * 1024;
const MAX_COMPONENT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LAUNCHD_BYTES: usize = 64 * 1024;
const UNSAFE_WRITE_BITS: u32 = 0o022;

const PAYLOAD_PATHS: [&str; 5] = [
    "ReposeUnlock.bundle/Contents/Info.plist",
    PLUGIN_EXECUTABLE_RELATIVE_PATH,
    "ReposeUnlock.bundle/Contents/_CodeSignature/CodeResources",
    SERVICE_RELATIVE_PATH,
    LAUNCHD_RELATIVE_PATH,
];

const PACKAGE_DIRECTORIES: [&str; 7] = [
    "",
    "ReposeUnlock.bundle",
    "ReposeUnlock.bundle/Contents",
    "ReposeUnlock.bundle/Contents/MacOS",
    "ReposeUnlock.bundle/Contents/_CodeSignature",
    "bin",
    "launchd",
];

const LAUNCHD_TEMPLATE: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>ai.repose.unlockd</string>
<key>ProgramArguments</key><array><string>/Library/PrivilegedHelperTools/ai.repose.unlockd</string></array>
<key>UserName</key><string>root</string>
<key>GroupName</key><string>wheel</string>
<key>Umask</key><integer>63</integer>
<key>Sockets</key><dict><key>ConsumeSocket</key><dict>
<key>SockPathName</key><string>/var/run/ai.repose.unlockd/consume.sock</string>
<key>SockType</key><string>stream</string>
<key>SockPassive</key><true/>
<key>SockPathOwner</key><integer>0</integer>
<key>SockPathGroup</key><integer>0</integer>
<key>SockPathMode</key><integer>384</integer>
</dict></dict>
</dict></plist>
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignaturePolicy {
    DevelopmentAdHoc,
    PinnedDeveloperId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VerificationPolicy {
    expected_uid: u32,
    signature: SignaturePolicy,
}

impl VerificationPolicy {
    #[must_use]
    pub const fn development(expected_uid: u32) -> Self {
        Self {
            expected_uid,
            signature: SignaturePolicy::DevelopmentAdHoc,
        }
    }
}

pub trait ArtifactAttestor {
    fn verify_plugin_signature(
        &mut self,
        bundle: &Path,
        policy: SignaturePolicy,
    ) -> Result<(), ArtifactError>;

    fn verify_service_signature(
        &mut self,
        executable: &Path,
        policy: SignaturePolicy,
    ) -> Result<(), ArtifactError>;
}

#[derive(Debug)]
pub struct InspectedDevelopmentPackage {
    root: PathBuf,
    root_handle: File,
    digests: BTreeMap<String, String>,
    signature: SignaturePolicy,
}

impl InspectedDevelopmentPackage {
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn digest(&self, relative_path: &str) -> Option<&str> {
        self.digests.get(relative_path).map(String::as_str)
    }

    #[must_use]
    pub const fn signature_policy(&self) -> SignaturePolicy {
        self.signature
    }

    pub fn root_identity(&self) -> io::Result<(u64, u64)> {
        let metadata = self.root_handle.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }
}

/// Capability required by the transaction backend for production install.
/// There is intentionally no public constructor in Task 7.
#[derive(Debug)]
pub struct ProductionSealedArtifacts {
    _private: (),
}

pub fn inspect_development_package(
    root: &Path,
    expected_uid: u32,
    attestor: &mut impl ArtifactAttestor,
) -> Result<InspectedDevelopmentPackage, ArtifactError> {
    let policy = VerificationPolicy::development(expected_uid);
    validate_root(root, policy.expected_uid)?;
    let discovered = walk_package(root, policy.expected_uid)?;
    let expected = expected_entries();
    if let Some(relative) = discovered.difference(&expected).next() {
        return Err(ArtifactError::UnexpectedEntry {
            path: relative.clone(),
        });
    }
    if let Some(relative) = expected.difference(&discovered).next() {
        return Err(ArtifactError::MissingEntry {
            path: relative.clone(),
        });
    }

    let manifest_path = root.join(MANIFEST_NAME);
    let manifest =
        read_bounded_regular_file(&manifest_path, policy.expected_uid, MAX_MANIFEST_BYTES)?;
    let manifest = String::from_utf8(manifest).map_err(|_| ArtifactError::MalformedManifest)?;
    let (mode, digests) = parse_manifest(&manifest)?;
    if mode != DEVELOPMENT_MODE {
        return Err(ArtifactError::PackageMode {
            expected: DEVELOPMENT_MODE,
            actual: mode.to_owned(),
        });
    }

    for relative in PAYLOAD_PATHS {
        let expected_digest =
            digests
                .get(relative)
                .ok_or_else(|| ArtifactError::MissingDigest {
                    path: relative.to_owned(),
                })?;
        let actual = hash_regular_file(&root.join(relative), policy.expected_uid)?;
        if &actual != expected_digest {
            return Err(ArtifactError::HashMismatch {
                path: relative.to_owned(),
            });
        }
    }
    if digests.len() != PAYLOAD_PATHS.len() {
        let unexpected = digests
            .keys()
            .find(|path| !PAYLOAD_PATHS.contains(&path.as_str()))
            .cloned()
            .unwrap_or_else(|| "duplicate manifest entry".to_owned());
        return Err(ArtifactError::UnexpectedDigest { path: unexpected });
    }

    validate_launchd_template(&read_bounded_regular_file(
        &root.join(LAUNCHD_RELATIVE_PATH),
        policy.expected_uid,
        MAX_LAUNCHD_BYTES as u64,
    )?)?;
    attestor.verify_plugin_signature(&root.join(PLUGIN_RELATIVE_PATH), policy.signature)?;
    attestor.verify_service_signature(&root.join(SERVICE_RELATIVE_PATH), policy.signature)?;

    let mut root_options = File::options();
    root_options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let root_handle = root_options.open(root).map_err(io_error)?;
    Ok(InspectedDevelopmentPackage {
        root: root.to_path_buf(),
        root_handle,
        digests,
        signature: policy.signature,
    })
}

/// Production apply gate. The path is intentionally not opened while the
/// capability is unavailable, so a root invocation cannot mutate or execute
/// anything from an untrusted source directory.
pub fn verify_apply_artifacts(_: &Path) -> Result<ProductionSealedArtifacts, ArtifactError> {
    Err(ArtifactError::ProductionGateClosed)
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

mod package_fs;

use package_fs::*;

#[derive(Debug)]
pub enum ArtifactError {
    Io(io::Error),
    ProductionGateClosed,
    UnsafeRoot,
    ManifestTooLarge,
    MalformedManifest,
    UnsafeManifestPath {
        path: String,
    },
    DuplicateDigest {
        path: String,
    },
    MissingDigest {
        path: String,
    },
    UnexpectedDigest {
        path: String,
    },
    HashMismatch {
        path: String,
    },
    PackageMode {
        expected: &'static str,
        actual: String,
    },
    WrongOwner {
        path: String,
        expected: u32,
        actual: u32,
    },
    UnsafeMode {
        path: String,
        mode: u32,
    },
    Symlink {
        path: String,
    },
    HardLink {
        path: String,
    },
    SpecialFile {
        path: String,
    },
    ComponentTooLarge {
        path: String,
    },
    ChangedDuringVerification {
        path: String,
    },
    MissingEntry {
        path: String,
    },
    UnexpectedEntry {
        path: String,
    },
    InvalidLaunchdTemplate {
        reason: &'static str,
    },
    Attestation(String),
}

impl Display for ArtifactError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => formatter.write_str("artifact I/O failed"),
            Self::ProductionGateClosed => formatter.write_str(
                "production apply gate is closed until Task 8 pins a signed service measurement",
            ),
            Self::UnsafeRoot => {
                formatter.write_str("artifact root is not a safe absolute directory")
            }
            Self::ManifestTooLarge => formatter.write_str("artifact manifest is too large"),
            Self::MalformedManifest => formatter.write_str("artifact manifest is malformed"),
            Self::UnsafeManifestPath { path } => write!(formatter, "unsafe manifest path: {path}"),
            Self::DuplicateDigest { path } => write!(formatter, "duplicate digest: {path}"),
            Self::MissingDigest { path } => write!(formatter, "missing digest: {path}"),
            Self::UnexpectedDigest { path } => write!(formatter, "unexpected digest: {path}"),
            Self::HashMismatch { path } => write!(formatter, "hash mismatch: {path}"),
            Self::PackageMode { expected, actual } => {
                write!(formatter, "package mode {actual} is not {expected}")
            }
            Self::WrongOwner { path, .. } => write!(formatter, "wrong artifact owner: {path}"),
            Self::UnsafeMode { path, .. } => write!(formatter, "unsafe artifact mode: {path}"),
            Self::Symlink { path } => write!(formatter, "artifact symlink: {path}"),
            Self::HardLink { path } => write!(formatter, "artifact hard link: {path}"),
            Self::SpecialFile { path } => write!(formatter, "artifact special file: {path}"),
            Self::ComponentTooLarge { path } => write!(formatter, "artifact too large: {path}"),
            Self::ChangedDuringVerification { path } => {
                write!(formatter, "artifact changed during verification: {path}")
            }
            Self::MissingEntry { path } => write!(formatter, "artifact missing: {path}"),
            Self::UnexpectedEntry { path } => write!(formatter, "unexpected artifact: {path}"),
            Self::InvalidLaunchdTemplate { reason } => {
                write!(formatter, "invalid fixed launchd template: {reason}")
            }
            Self::Attestation(reason) => write!(formatter, "artifact attestation failed: {reason}"),
        }
    }
}

impl Error for ArtifactError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
