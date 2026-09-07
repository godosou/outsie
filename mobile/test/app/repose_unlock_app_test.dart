import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/app/repose_unlock_app.dart';
import 'package:repose_unlock/native/native_models.dart';

import '../support/fake_native_gateway.dart';

void main() {
  testWidgets('identifies the realme Android 16 target and shared UI', (
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

    expect(find.text('Repose Phone Key'), findsOneWidget);
    expect(find.textContaining('realme GT5 Pro'), findsWidgets);
    expect(find.textContaining('Android 16 (API 36)'), findsOneWidget);
    expect(find.textContaining('Android and iOS'), findsOneWidget);
    expect(find.text('Native capabilities ready'), findsOneWidget);
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
      expect(find.text('realme GT5 Pro'), findsOneWidget);
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
      tester.getSemantics(find.text('Native capabilities ready')),
      matchesSemantics(
        label: 'Capability status\nNative capabilities ready',
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
