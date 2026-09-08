import 'package:pigeon/pigeon.dart';

enum NativeCompanionCapability {
  ready,
  associationNotConfigured,
  bluetoothUnavailable,
  secureHardwareUnavailable,
  backgroundExecutionUnavailable,
  unsupportedPlatform,
  nativeBridgeUnavailable,
}

enum NativeCompanionPlatform { android, ios }

enum NativeCalibrationStep { near, far }

enum NativeCalibrationPhase {
  idle,
  collectingNear,
  collectingFar,
  complete,
  overlapRejected,
  unavailable,
  failed,
}

class NativePairingSession {
  NativePairingSession({
    required this.sessionId,
    required this.deviceName,
    required this.expiresAtEpochMillis,
  });

  String sessionId;
  String deviceName;
  int expiresAtEpochMillis;
}

class NativePairedDevice {
  NativePairedDevice({
    required this.id,
    required this.displayName,
    required this.platform,
  });

  String id;
  String displayName;
  NativeCompanionPlatform platform;
}

class NativeCalibrationSnapshot {
  NativeCalibrationSnapshot({required this.phase});

  NativeCalibrationPhase phase;
}

class NativeUnlockSnapshot {
  NativeUnlockSnapshot({
    required this.capability,
    required this.devices,
    required this.calibration,
    this.pendingPairing,
  });

  NativeCompanionCapability capability;
  List<NativePairedDevice> devices;
  NativeCalibrationSnapshot calibration;
  NativePairingSession? pendingPairing;
}

class NativeDiagnostics {
  NativeDiagnostics({required this.summary});

  String summary;
}

@HostApi()
abstract class ReposeUnlockHostApi {
  @TaskQueue(type: TaskQueueType.serialBackgroundThread)
  NativeUnlockSnapshot getSnapshot();

  @async
  @TaskQueue(type: TaskQueueType.serialBackgroundThread)
  void requestCompanionAssociation();

  @TaskQueue(type: TaskQueueType.serialBackgroundThread)
  NativePairingSession beginPairing(String qrPayload);

  @TaskQueue(type: TaskQueueType.serialBackgroundThread)
  void confirmPairing(String sessionId);

  @TaskQueue(type: TaskQueueType.serialBackgroundThread)
  void startCalibration();

  @TaskQueue(type: TaskQueueType.serialBackgroundThread)
  void submitCalibrationStep(NativeCalibrationStep step);

  @TaskQueue(type: TaskQueueType.serialBackgroundThread)
  void revokeDevice(String deviceId);

  @TaskQueue(type: TaskQueueType.serialBackgroundThread)
  NativeDiagnostics getDiagnostics();
}
