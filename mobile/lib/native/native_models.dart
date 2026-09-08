enum CompanionCapability {
  ready,
  loading,
  associationNotConfigured,
  bluetoothUnavailable,
  secureHardwareUnavailable,
  backgroundExecutionUnavailable,
  unsupportedPlatform,
  nativeBridgeUnavailable,
}

enum CompanionPlatform { android, ios }

enum PairingPhase {
  idle,
  starting,
  awaitingConfirmation,
  confirming,
  paired,
  expired,
  failed,
}

enum CalibrationStep { near, far }

enum CalibrationPhase {
  idle,
  collectingNear,
  collectingFar,
  complete,
  overlapRejected,
  unavailable,
  failed,
}

enum NativeErrorCode {
  qrExpired,
  qrAlreadyUsed,
  pairingNotAccepted,
  capabilityUnavailable,
  unsupported,
  calibrationOutOfOrder,
  calibrationOverlap,
  deviceNotFound,
  bridgeUnavailable,
  associationBusy,
  bluetoothPermissionDenied,
  associationCancelled,
  activityUnavailable,
  associationDiscoveryFailed,
  associationConfigurationFailed,
  unknown,
}

class NativeGatewayException implements Exception {
  const NativeGatewayException(this.code, {this.safeMessage});

  final NativeErrorCode code;
  final String? safeMessage;

  @override
  String toString() => 'NativeGatewayException(${code.name})';
}

class PairingSession {
  const PairingSession({
    required this.sessionId,
    required this.deviceName,
    required this.expiresAt,
  });

  final String sessionId;
  final String deviceName;
  final DateTime expiresAt;
}

class PairedDevice {
  const PairedDevice({
    required this.id,
    required this.displayName,
    required this.platform,
  });

  final String id;
  final String displayName;
  final CompanionPlatform platform;
}

class CalibrationSnapshot {
  const CalibrationSnapshot({this.phase = CalibrationPhase.idle});

  final CalibrationPhase phase;
}

class UnlockSnapshot {
  UnlockSnapshot({
    required this.capability,
    required Iterable<PairedDevice> devices,
    required this.calibration,
    this.pendingPairing,
  }) : devices = List<PairedDevice>.unmodifiable(devices);

  factory UnlockSnapshot.empty() => UnlockSnapshot(
    capability: CompanionCapability.loading,
    devices: const <PairedDevice>[],
    calibration: const CalibrationSnapshot(),
  );

  final CompanionCapability capability;
  final List<PairedDevice> devices;
  final CalibrationSnapshot calibration;
  final PairingSession? pendingPairing;
}

class Diagnostics {
  const Diagnostics({required this.summary});

  final String summary;
}
