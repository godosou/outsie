import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/features/devices/device_controller.dart';
import 'package:repose_unlock/native/native_models.dart';

import 'support/fake_native_gateway.dart';

void main() {
  group('CapabilityGate', () {
    test('enables pairing and calibration only when native is ready', () {
      final gate = CapabilityGate.from(CompanionCapability.ready);

      expect(gate.canAssociate, isFalse);
      expect(gate.canPair, isTrue);
      expect(gate.canCalibrate, isTrue);
      expect(gate.message, isNull);
    });

    for (final capability in CompanionCapability.values.where(
      (value) => value != CompanionCapability.ready,
    )) {
      test('fails closed with a visible state for ${capability.name}', () {
        final gate = CapabilityGate.from(capability);

        expect(gate.canPair, isFalse);
        expect(gate.canCalibrate, isFalse);
        expect(gate.message, isNotEmpty);
      });
    }

    test('exposes only system association before a Mac is configured', () {
      final gate = CapabilityGate.from(
        CompanionCapability.associationNotConfigured,
      );

      expect(gate.canAssociate, isTrue);
      expect(gate.canPair, isFalse);
      expect(gate.canCalibrate, isFalse);
      expect(
        gate.message,
        'Connect this phone to a Repose Mac before scanning the one-time '
        'pairing QR code.',
      );
    });
  });

  group('DeviceController', () {
    test('hydrates the complete native snapshot after a UI restart', () async {
      final snapshot = UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[
          PairedDevice(
            id: 'phone-1',
            displayName: 'realme GT5 Pro',
            platform: CompanionPlatform.android,
          ),
        ],
        calibration: const CalibrationSnapshot(
          phase: CalibrationPhase.collectingFar,
        ),
        pendingPairing: PairingSession(
          sessionId: 'pending-confirmation',
          deviceName: 'realme GT5 Pro',
          expiresAt: DateTime.utc(2026, 9, 8, 10, 2),
        ),
      );
      final gateway = FakeNativeGateway(snapshot: snapshot);
      final controller = DeviceController(gateway: gateway);

      await controller.hydrate();

      expect(gateway.snapshotReadCount, 1);
      expect(controller.state.snapshot, same(snapshot));
      expect(controller.state.devices.single.displayName, 'realme GT5 Pro');
      expect(
        controller.state.calibration.phase,
        CalibrationPhase.collectingFar,
      );
      expect(
        controller.state.snapshot?.pendingPairing?.sessionId,
        'pending-confirmation',
      );
      expect(controller.state.isHydrated, isTrue);
    });

    test('system association refreshes authoritative native state', () async {
      final gateway = FakeNativeGateway(
        snapshot: UnlockSnapshot(
          capability: CompanionCapability.associationNotConfigured,
          devices: const <PairedDevice>[],
          calibration: const CalibrationSnapshot(),
        ),
      );
      gateway.onRequestCompanionAssociation = () {
        gateway.snapshot = UnlockSnapshot(
          capability: CompanionCapability.backgroundExecutionUnavailable,
          devices: const <PairedDevice>[],
          calibration: const CalibrationSnapshot(),
        );
      };
      final controller = DeviceController(gateway: gateway);
      await controller.hydrate();

      expect(await controller.requestCompanionAssociation(), isTrue);

      expect(gateway.associationRequestCount, 1);
      expect(gateway.snapshotReadCount, 2);
      expect(
        controller.state.snapshot?.capability,
        CompanionCapability.backgroundExecutionUnavailable,
      );
      expect(
        controller.state.message,
        'Android connection saved. Next, scan the one-time pairing QR code '
        'shown on your Mac.',
      );
    });

    test(
      'denied association remains retryable but never opens pairing',
      () async {
        final gateway =
            FakeNativeGateway(
                snapshot: UnlockSnapshot(
                  capability: CompanionCapability.associationNotConfigured,
                  devices: const <PairedDevice>[],
                  calibration: const CalibrationSnapshot(),
                ),
              )
              ..associationError = const NativeGatewayException(
                NativeErrorCode.bluetoothPermissionDenied,
                safeMessage:
                    'Nearby devices access is required to find your Mac.',
              );
        final controller = DeviceController(gateway: gateway);
        await controller.hydrate();

        expect(await controller.requestCompanionAssociation(), isFalse);

        expect(controller.state.gate.canAssociate, isTrue);
        expect(controller.state.gate.canPair, isFalse);
        expect(controller.state.message, contains('Nearby devices'));
      },
    );

    test(
      'revokes a device natively before removing it from UI state',
      () async {
        final gateway = FakeNativeGateway(
          snapshot: UnlockSnapshot(
            capability: CompanionCapability.ready,
            devices: const <PairedDevice>[
              PairedDevice(
                id: 'phone-1',
                displayName: 'realme GT5 Pro',
                platform: CompanionPlatform.android,
              ),
            ],
            calibration: const CalibrationSnapshot(),
          ),
        );
        gateway.onRevoke = (_) {
          gateway.snapshot = UnlockSnapshot(
            capability: CompanionCapability.ready,
            devices: <PairedDevice>[],
            calibration: CalibrationSnapshot(),
          );
        };
        final controller = DeviceController(gateway: gateway);
        await controller.hydrate();

        expect(await controller.revokeDevice('phone-1'), isTrue);

        expect(gateway.revokedDeviceIds, <String>['phone-1']);
        expect(gateway.snapshotReadCount, 2);
        expect(controller.state.devices, isEmpty);
      },
    );

    test('keeps the device visible when native revocation fails', () async {
      final gateway =
          FakeNativeGateway(
              snapshot: UnlockSnapshot(
                capability: CompanionCapability.ready,
                devices: <PairedDevice>[
                  PairedDevice(
                    id: 'phone-1',
                    displayName: 'realme GT5 Pro',
                    platform: CompanionPlatform.android,
                  ),
                ],
                calibration: CalibrationSnapshot(),
              ),
            )
            ..revokeError = const NativeGatewayException(
              NativeErrorCode.deviceNotFound,
            );
      final controller = DeviceController(gateway: gateway);
      await controller.hydrate();

      expect(await controller.revokeDevice('phone-1'), isFalse);

      expect(controller.state.devices.single.id, 'phone-1');
      expect(controller.state.message, isNotEmpty);
      expect(controller.state.gate.canPair, isFalse);
      expect(controller.state.gate.canCalibrate, isFalse);
    });

    test(
      'suppresses a duplicate revoke while native work is pending',
      () async {
        final completer = Completer<void>();
        final gateway = FakeNativeGateway(
          snapshot: UnlockSnapshot(
            capability: CompanionCapability.ready,
            devices: <PairedDevice>[
              PairedDevice(
                id: 'phone-1',
                displayName: 'realme GT5 Pro',
                platform: CompanionPlatform.android,
              ),
            ],
            calibration: CalibrationSnapshot(),
          ),
        )..onRevoke = (_) => completer.future;
        final controller = DeviceController(gateway: gateway);
        await controller.hydrate();

        final first = controller.revokeDevice('phone-1');
        expect(controller.state.revokingDeviceIds, <String>{'phone-1'});
        expect(await controller.revokeDevice('phone-1'), isFalse);
        completer.complete();
        await first;

        expect(gateway.revokedDeviceIds, <String>['phone-1']);
      },
    );

    test(
      'serializes A and B revokes so reverse completion cannot race',
      () async {
        final pending = Completer<void>();
        final gateway = FakeNativeGateway(
          snapshot: _readySnapshot(
            devices: const <PairedDevice>[_phoneA, _phoneB],
          ),
        );
        gateway.onRevoke = (_) async {
          await pending.future;
          gateway.snapshot = _readySnapshot(
            devices: const <PairedDevice>[_phoneB],
          );
        };
        final controller = DeviceController(gateway: gateway);
        await controller.hydrate();

        final revokeA = controller.revokeDevice('phone-a');
        final revokeB = controller.revokeDevice('phone-b');
        final inFlight = controller.state.revokingDeviceIds;
        pending.complete();

        expect(await revokeA, isTrue);
        expect(await revokeB, isFalse);
        expect(inFlight, <String>{'phone-a'});
        expect(gateway.revokedDeviceIds, <String>['phone-a']);
        expect(controller.state.devices, const <PairedDevice>[_phoneB]);
        expect(controller.state.revokingDeviceIds, isEmpty);
      },
    );

    test('snapshot failure remains fail-closed', () async {
      final gateway = FakeNativeGateway()
        ..snapshotError = const NativeGatewayException(
          NativeErrorCode.bridgeUnavailable,
        );
      final controller = DeviceController(gateway: gateway);

      await controller.hydrate();

      expect(controller.state.isHydrated, isFalse);
      expect(controller.state.gate.canPair, isFalse);
      expect(controller.state.gate.canCalibrate, isFalse);
      expect(controller.state.message, isNotEmpty);
    });

    test('failed refresh closes a previously ready capability gate', () async {
      final gateway = FakeNativeGateway(
        snapshot: _readySnapshot(devices: const <PairedDevice>[_phoneA]),
      );
      final controller = DeviceController(gateway: gateway);
      expect(await controller.hydrate(), isTrue);
      expect(controller.state.gate.canPair, isTrue);
      gateway.snapshotError = const NativeGatewayException(
        NativeErrorCode.bridgeUnavailable,
        safeMessage: 'Native refresh unavailable.',
      );

      expect(await controller.hydrate(), isFalse);

      expect(controller.state.devices, const <PairedDevice>[_phoneA]);
      expect(controller.state.gate.canPair, isFalse);
      expect(controller.state.gate.canCalibrate, isFalse);
      expect(controller.state.message, 'Native refresh unavailable.');
    });

    test('newer failed refresh fences an older ready response', () async {
      final older = Completer<UnlockSnapshot>();
      final newer = Completer<UnlockSnapshot>();
      var request = 0;
      final gateway = FakeNativeGateway(snapshot: _readySnapshot());
      final controller = DeviceController(gateway: gateway);
      expect(await controller.hydrate(), isTrue);
      gateway.onGetSnapshot = () =>
          request++ == 0 ? older.future : newer.future;

      final olderRefresh = controller.hydrate();
      final newerRefresh = controller.hydrate();
      newer.completeError(
        const NativeGatewayException(NativeErrorCode.bridgeUnavailable),
      );
      expect(await newerRefresh, isFalse);
      older.complete(_readySnapshot(devices: const <PairedDevice>[_phoneA]));

      expect(await olderRefresh, isFalse);
      expect(controller.state.gate.canPair, isFalse);
      expect(controller.state.devices, isEmpty);
      expect(controller.state.message, isNotEmpty);
    });

    test(
      'late snapshot after dispose is ignored without notification',
      () async {
        final pending = Completer<UnlockSnapshot>();
        final gateway = FakeNativeGateway()
          ..onGetSnapshot = () => pending.future;
        final controller = DeviceController(gateway: gateway);
        var notifications = 0;
        controller.addListener(() => notifications += 1);

        final hydration = controller.hydrate();
        expect(notifications, 1);
        controller.dispose();
        pending.complete(_readySnapshot());

        expect(await hydration, isFalse);
        expect(notifications, 1);
        expect(controller.state.isLoading, isTrue);
      },
    );

    test('late revoke after dispose skips refresh and notification', () async {
      final pending = Completer<void>();
      final gateway = FakeNativeGateway(
        snapshot: _readySnapshot(devices: const <PairedDevice>[_phoneA]),
      )..onRevoke = (_) => pending.future;
      final controller = DeviceController(gateway: gateway);
      expect(await controller.hydrate(), isTrue);
      var notifications = 0;
      controller.addListener(() => notifications += 1);

      final revocation = controller.revokeDevice('phone-a');
      expect(notifications, 1);
      controller.dispose();
      pending.complete();

      expect(await revocation, isFalse);
      expect(notifications, 1);
      expect(controller.state.revokingDeviceIds, <String>{'phone-a'});
      expect(gateway.snapshotReadCount, 1);
    });
  });
}

const _phoneA = PairedDevice(
  id: 'phone-a',
  displayName: 'Phone A',
  platform: CompanionPlatform.android,
);

const _phoneB = PairedDevice(
  id: 'phone-b',
  displayName: 'Phone B',
  platform: CompanionPlatform.ios,
);

UnlockSnapshot _readySnapshot({
  Iterable<PairedDevice> devices = const <PairedDevice>[],
}) => UnlockSnapshot(
  capability: CompanionCapability.ready,
  devices: devices,
  calibration: const CalibrationSnapshot(),
);
