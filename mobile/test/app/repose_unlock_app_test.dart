import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/app/repose_unlock_app.dart';
import 'package:repose_unlock/native/native_models.dart';

import '../support/fake_native_gateway.dart';

void main() {
  testWidgets('ready phone key opens on a car-key status dashboard', (
    tester,
  ) async {
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[
          PairedDevice(
            id: 'realme-1',
            displayName: 'realme GT5 Pro',
            platform: CompanionPlatform.android,
          ),
        ],
        calibration: const CalibrationSnapshot(
          phase: CalibrationPhase.complete,
        ),
      ),
    );

    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('phoneKeyHero')), findsOneWidget);
    expect(find.byKey(const Key('proximityOrb')), findsOneWidget);
    expect(find.text('SETUP COMPLETE'), findsOneWidget);
    expect(find.text('Phone key setup complete'), findsOneWidget);
    expect(find.text('Ready to unlock'), findsNothing);
    expect(find.text('Protected by on-device security'), findsOneWidget);
  });

  testWidgets('paired phone key prioritizes calibration over developer copy', (
    tester,
  ) async {
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[
          PairedDevice(
            id: 'realme-1',
            displayName: 'realme GT5 Pro',
            platform: CompanionPlatform.android,
          ),
        ],
        calibration: const CalibrationSnapshot(),
      ),
    );

    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    expect(find.text('Repose Key'), findsOneWidget);
    expect(find.byKey(const Key('phoneKeyHero')), findsOneWidget);
    expect(find.byKey(const Key('setupProgress')), findsOneWidget);
    expect(find.text('CALIBRATION NEEDED'), findsOneWidget);
    expect(find.text('Set your unlock distance'), findsOneWidget);
    expect(
      find.descendant(
        of: find.byKey(const Key('phoneKeyHero')),
        matching: find.text('realme GT5 Pro'),
      ),
      findsOneWidget,
    );
    expect(find.textContaining('Android 16'), findsNothing);
    expect(find.textContaining('Android and iOS'), findsNothing);
    expect(find.textContaining('Native capabilities'), findsNothing);
  });

  testWidgets('unsupported capability is visible and actions fail closed', (
    tester,
  ) async {
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.unsupportedPlatform,
        devices: const <PairedDevice>[],
        calibration: const CalibrationSnapshot(),
      ),
    );

    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    expect(find.textContaining('unsupported'), findsOneWidget);
    expect(find.text('PHONE KEY OFFLINE'), findsOneWidget);
    expect(
      find.text('No background polling fallback will be enabled.'),
      findsOneWidget,
    );
    expect(find.text('Protected by on-device security'), findsNothing);
    expect(
      tester
          .widget<FilledButton>(find.byKey(const Key('beginPairingButton')))
          .onPressed,
      isNull,
    );
    expect(
      tester
          .widget<FilledButton>(find.byKey(const Key('startCalibrationButton')))
          .onPressed,
      isNull,
    );
  });

  testWidgets(
    'first pairing exposes device name before explicit confirmation',
    (tester) async {
      final gateway = FakeNativeGateway(
        snapshot: UnlockSnapshot(
          capability: CompanionCapability.ready,
          devices: const <PairedDevice>[],
          calibration: const CalibrationSnapshot(),
        ),
        pairingSession: PairingSession(
          sessionId: 'ui-first-pair',
          deviceName: 'realme GT5 Pro',
          expiresAt: DateTime.utc(2099),
        ),
      );
      gateway.onConfirmPairing = (_) {
        gateway.snapshot = UnlockSnapshot(
          capability: CompanionCapability.ready,
          devices: const <PairedDevice>[
            PairedDevice(
              id: 'realme-1',
              displayName: 'realme GT5 Pro',
              platform: CompanionPlatform.android,
            ),
          ],
          calibration: const CalibrationSnapshot(),
        );
      };
      await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
      await tester.pumpAndSettle();

      await tester.enterText(
        find.byKey(const Key('pairingQrField')),
        'repose://pair/opaque-token',
      );
      await tester.tap(find.byKey(const Key('beginPairingButton')));
      await tester.pumpAndSettle();

      expect(find.textContaining('Confirm realme GT5 Pro'), findsOneWidget);
      expect(gateway.confirmedSessionIds, isEmpty);

      final confirmButton = find.byKey(const Key('confirmPairingButton'));
      await tester.ensureVisible(confirmButton);
      await tester.tap(confirmButton);
      await tester.pumpAndSettle();

      expect(gateway.confirmedSessionIds, <String>['ui-first-pair']);
      expect(find.text('Pairing confirmed'), findsOneWidget);
      expect(find.text('realme GT5 Pro'), findsWidgets);
      expect(gateway.snapshotReadCount, 2);
      expect(
        tester
            .widget<FilledButton>(
              find.byKey(const Key('startCalibrationButton')),
            )
            .onPressed,
        isNotNull,
      );
    },
  );

  testWidgets('paired dashboard can reveal an add-another-key flow', (
    tester,
  ) async {
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[
          PairedDevice(
            id: 'realme-1',
            displayName: 'realme GT5 Pro',
            platform: CompanionPlatform.android,
          ),
        ],
        calibration: const CalibrationSnapshot(
          phase: CalibrationPhase.complete,
        ),
      ),
    );
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('pairingQrField')), findsNothing);
    final addAnother = find.byKey(const Key('addAnotherKeyButton'));
    await tester.ensureVisible(addAnother);
    await tester.tap(addAnother);
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('pairingQrField')), findsOneWidget);
    expect(find.byKey(const Key('beginPairingButton')), findsOneWidget);
  });

  testWidgets('overlapping calibration samples show an explicit retry', (
    tester,
  ) async {
    final gateway =
        FakeNativeGateway(
            snapshot: UnlockSnapshot(
              capability: CompanionCapability.ready,
              devices: const <PairedDevice>[
                PairedDevice(
                  id: 'realme-1',
                  displayName: 'realme GT5 Pro',
                  platform: CompanionPlatform.android,
                ),
              ],
              calibration: const CalibrationSnapshot(),
            ),
          )
          ..calibrationSubmissionErrors[CalibrationStep.far] =
              const NativeGatewayException(NativeErrorCode.calibrationOverlap);
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    final startButton = find.byKey(const Key('startCalibrationButton'));
    await tester.ensureVisible(startButton);
    await tester.tap(startButton);
    await tester.pumpAndSettle();
    final nearButton = find.byKey(const Key('submitNearButton'));
    await tester.ensureVisible(nearButton);
    await tester.tap(nearButton);
    await tester.pumpAndSettle();
    final farButton = find.byKey(const Key('submitFarButton'));
    await tester.ensureVisible(farButton);
    await tester.tap(farButton);
    await tester.pumpAndSettle();

    expect(find.textContaining('overlap'), findsOneWidget);
    expect(find.text('Retry calibration'), findsOneWidget);
    expect(find.text('Distance calibration complete'), findsNothing);
  });

  testWidgets('completed calibration promotes the hero without a restart', (
    tester,
  ) async {
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[
          PairedDevice(
            id: 'realme-1',
            displayName: 'realme GT5 Pro',
            platform: CompanionPlatform.android,
          ),
        ],
        calibration: const CalibrationSnapshot(),
      ),
    );
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    final start = find.byKey(const Key('startCalibrationButton'));
    await tester.ensureVisible(start);
    await tester.tap(start);
    await tester.pumpAndSettle();
    final near = find.byKey(const Key('submitNearButton'));
    await tester.ensureVisible(near);
    await tester.tap(near);
    await tester.pumpAndSettle();
    final far = find.byKey(const Key('submitFarButton'));
    await tester.ensureVisible(far);
    await tester.tap(far);
    await tester.pumpAndSettle();

    expect(find.text('Distance calibration complete.'), findsOneWidget);
    expect(find.text('SETUP COMPLETE'), findsOneWidget);
    expect(find.text('Phone key setup complete'), findsOneWidget);
    expect(find.text('Ready to unlock'), findsNothing);
  });

  testWidgets('pair confirmation refresh failure closes the visible gate', (
    tester,
  ) async {
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[],
        calibration: const CalibrationSnapshot(),
      ),
      pairingSession: PairingSession(
        sessionId: 'refresh-fails',
        deviceName: 'realme GT5 Pro',
        expiresAt: DateTime.utc(2099),
      ),
    );
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('pairingQrField')),
      'repose://pair/refresh-fails',
    );
    await tester.tap(find.byKey(const Key('beginPairingButton')));
    await tester.pumpAndSettle();
    gateway.snapshotError = const NativeGatewayException(
      NativeErrorCode.bridgeUnavailable,
    );
    final confirm = find.byKey(const Key('confirmPairingButton'));
    await tester.ensureVisible(confirm);

    await tester.tap(confirm);
    await tester.pumpAndSettle();

    expect(find.text('Pairing confirmed'), findsNothing);
    expect(find.textContaining('refresh'), findsWidgets);
    expect(
      tester
          .widget<FilledButton>(find.byKey(const Key('beginPairingButton')))
          .onPressed,
      isNull,
    );
    expect(
      tester
          .widget<FilledButton>(find.byKey(const Key('startCalibrationButton')))
          .onPressed,
      isNull,
    );
  });

  testWidgets('restores pending native workflow state after UI restart', (
    tester,
  ) async {
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[
          PairedDevice(
            id: 'realme-1',
            displayName: 'realme GT5 Pro',
            platform: CompanionPlatform.android,
          ),
        ],
        calibration: const CalibrationSnapshot(
          phase: CalibrationPhase.collectingFar,
        ),
        pendingPairing: PairingSession(
          sessionId: 'restored-session',
          deviceName: 'realme GT5 Pro',
          expiresAt: DateTime.utc(2099),
        ),
      ),
    );

    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    expect(find.textContaining('Confirm realme GT5 Pro'), findsOneWidget);
    expect(find.byKey(const Key('confirmPairingButton')), findsOneWidget);
    expect(find.byKey(const Key('submitFarButton')), findsOneWidget);
    expect(gateway.pairingPayloads, isEmpty);
  });

  testWidgets('pending confirmation is disabled on unsupported snapshot', (
    tester,
  ) async {
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.unsupportedPlatform,
        devices: const <PairedDevice>[],
        calibration: const CalibrationSnapshot(),
        pendingPairing: PairingSession(
          sessionId: 'blocked-confirm',
          deviceName: 'realme GT5 Pro',
          expiresAt: DateTime.utc(2099),
        ),
      ),
    );

    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    expect(
      tester
          .widget<FilledButton>(find.byKey(const Key('confirmPairingButton')))
          .onPressed,
      isNull,
    );
    expect(gateway.confirmedSessionIds, isEmpty);
  });

  testWidgets('calibration actions are disabled while native work is pending', (
    tester,
  ) async {
    final pending = Completer<void>();
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[
          PairedDevice(
            id: 'realme-1',
            displayName: 'realme GT5 Pro',
            platform: CompanionPlatform.android,
          ),
        ],
        calibration: const CalibrationSnapshot(),
      ),
    )..onStartCalibration = () => pending.future;
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();
    final startButton = find.byKey(const Key('startCalibrationButton'));
    await tester.ensureVisible(startButton);

    await tester.tap(startButton);
    await tester.pump();

    expect(tester.widget<FilledButton>(startButton).onPressed, isNull);
    expect(gateway.calibrationStartCount, 1);
    pending.complete();
    await tester.pumpAndSettle();
  });

  testWidgets('async capability status is announced as a live region', (
    tester,
  ) async {
    final semantics = tester.ensureSemantics();
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[],
        calibration: const CalibrationSnapshot(),
      ),
    );
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    expect(
      tester.getSemantics(find.text('Phone key setup is ready')),
      matchesSemantics(
        label: 'Phone key status\nPhone key setup is ready',
        textDirection: TextDirection.ltr,
        isLiveRegion: true,
      ),
    );
    semantics.dispose();
  });

  testWidgets('unavailable phone key reason is announced as a live region', (
    tester,
  ) async {
    final semantics = tester.ensureSemantics();
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.bluetoothUnavailable,
        devices: const <PairedDevice>[],
        calibration: const CalibrationSnapshot(),
      ),
    );
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    const reason = 'Bluetooth is unavailable. Turn it on to continue.';
    expect(
      tester.getSemantics(find.text(reason)),
      matchesSemantics(
        label: 'Phone key status\n$reason',
        textDirection: TextDirection.ltr,
        isLiveRegion: true,
      ),
    );
    semantics.dispose();
  });

  testWidgets('successful revocation is announced as a live region', (
    tester,
  ) async {
    final semantics = tester.ensureSemantics();
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[
          PairedDevice(
            id: 'realme-1',
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
        devices: const <PairedDevice>[],
        calibration: const CalibrationSnapshot(),
      );
    };
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();
    final revoke = find.byTooltip('Revoke realme GT5 Pro');
    await tester.ensureVisible(revoke);

    await tester.tap(revoke);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Revoke'));
    await tester.pumpAndSettle();

    final status = find.text('Phone key revoked.');
    expect(status, findsOneWidget);
    final statusSemantics = find.ancestor(
      of: status,
      matching: find.byType(Semantics),
    );
    expect(
      statusSemantics.evaluate().map(
        (element) => (element.widget as Semantics).properties.liveRegion,
      ),
      contains(true),
    );
    semantics.dispose();
  });

  testWidgets('revoking the last key clears stale calibration presentation', (
    tester,
  ) async {
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[
          PairedDevice(
            id: 'realme-1',
            displayName: 'realme GT5 Pro',
            platform: CompanionPlatform.android,
          ),
        ],
        calibration: const CalibrationSnapshot(
          phase: CalibrationPhase.complete,
        ),
      ),
    );
    gateway.onRevoke = (_) {
      gateway.snapshot = UnlockSnapshot(
        capability: CompanionCapability.ready,
        devices: const <PairedDevice>[],
        calibration: const CalibrationSnapshot(),
      );
    };
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    final revoke = find.byTooltip('Revoke realme GT5 Pro');
    await tester.ensureVisible(revoke);
    await tester.tap(revoke);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Revoke'));
    await tester.pumpAndSettle();

    expect(find.text('READY TO PAIR'), findsOneWidget);
    expect(find.text('No paired devices yet.'), findsOneWidget);
    expect(find.text('Distance calibration complete.'), findsNothing);
  });

  testWidgets('narrow phone remains usable at 300% text scale', (tester) async {
    await tester.binding.setSurfaceSize(const Size(360, 800));
    tester.platformDispatcher.textScaleFactorTestValue = 3;
    addTearDown(() async {
      tester.platformDispatcher.clearTextScaleFactorTestValue();
      await tester.binding.setSurfaceSize(null);
    });
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.bluetoothUnavailable,
        devices: const <PairedDevice>[],
        calibration: const CalibrationSnapshot(),
      ),
    );

    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();

    expect(tester.takeException(), isNull);
    expect(find.textContaining('Bluetooth is unavailable'), findsOneWidget);
    expect(find.byKey(const Key('beginPairingButton')), findsOneWidget);
  });
}
