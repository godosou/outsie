import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/app/repose_unlock_app.dart';
import 'package:repose_unlock/native/native_models.dart';

import '../support/fake_native_gateway.dart';

void main() {
  const channel = MethodChannel('ai.repose/system_settings');
  late Map<String, Object?> status;
  late List<String> actions;
  var fail = false;

  setUp(() {
    actions = [];
    fail = false;
    status = {
      'platform': 'android',
      'manufacturer': 'realme',
      'nearby': 'notRequired',
      'notifications': 'notRequired',
      'background': 'optimized',
      'powerSaver': false,
    };
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          if (call.method == 'getStatus') {
            if (fail) throw PlatformException(code: 'unavailable');
            return status;
          }
          actions.add(call.method);
          if (fail) throw PlatformException(code: 'unavailable');
          return true;
        });
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  Future<void> openSettings(WidgetTester tester) async {
    await tester.pumpWidget(ReposeUnlockApp(gateway: FakeNativeGateway()));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Permissions & background'));
    await tester.pumpAndSettle();
  }

  testWidgets('Chinese phone uses localized Mac brand and warm light palette', (
    tester,
  ) async {
    tester.platformDispatcher.localesTestValue = const [Locale('zh', 'CN')];
    addTearDown(tester.platformDispatcher.clearLocalesTestValue);
    await tester.pumpWidget(
      ReposeUnlockApp(
        gateway: FakeNativeGateway(
          snapshot: UnlockSnapshot(
            capability: CompanionCapability.backgroundExecutionUnavailable,
            devices: const [],
            calibration: const CalibrationSnapshot(),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('repose.'), findsOneWidget);
    expect(find.text('手机钥匙'), findsOneWidget);
    expect(find.byTooltip('权限与后台运行'), findsOneWidget);
    final theme = Theme.of(tester.element(find.byType(Scaffold).first));
    expect(theme.scaffoldBackgroundColor, const Color(0xfffbfcf9));
    expect(find.textContaining('允许后台运行后'), findsNothing);
    expect(find.textContaining('当前版本尚未开放'), findsWidgets);
  });

  testWidgets(
    'settings distinguish unused permissions from a denial without prompting',
    (tester) async {
      await openSettings(tester);
      expect(find.text('Nearby devices'), findsOneWidget);
      expect(find.text('Not requested in this version'), findsNWidgets(2));
      expect(find.text('Battery optimization is on'), findsOneWidget);
      expect(find.textContaining('realme'), findsOneWidget);
      expect(actions, isEmpty);
    },
  );

  testWidgets(
    'background explanation precedes user initiated system settings',
    (tester) async {
      await openSettings(tester);
      final action = find.byKey(const Key('backgroundSettingsButton'));
      await tester.ensureVisible(action);
      await tester.tap(action);
      await tester.pumpAndSettle();
      expect(find.text('Before opening Settings'), findsOneWidget);
      expect(find.textContaining('more battery'), findsOneWidget);
      expect(actions, isEmpty);
      await tester.tap(find.text('Not now'));
      await tester.pumpAndSettle();
      expect(actions, isEmpty);
      await tester.tap(action);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Open app settings'));
      await tester.pumpAndSettle();
      expect(actions, ['openAppSettings']);
    },
  );

  testWidgets(
    'denied permission has rationale and permanent denial opens settings',
    (tester) async {
      status['nearby'] = 'denied';
      await openSettings(tester);
      await tester.tap(find.byKey(const Key('nearbyPermissionButton')));
      await tester.pumpAndSettle();
      expect(find.text('Allow nearby devices?'), findsOneWidget);
      expect(actions, isEmpty);
      await tester.tap(find.text('Continue'));
      await tester.pumpAndSettle();
      expect(actions, ['requestNearby']);
      status['nearby'] = 'permanentlyDenied';
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
      await tester.pumpAndSettle();
      expect(find.text('Allow in Settings'), findsOneWidget);
      await tester.tap(find.byKey(const Key('nearbyPermissionButton')));
      await tester.pumpAndSettle();
      expect(actions, ['requestNearby', 'openAppSettings']);
    },
  );

  testWidgets('returning from settings rechecks background state', (
    tester,
  ) async {
    await openSettings(tester);
    status['background'] = 'restricted';
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pumpAndSettle();
    expect(find.text('Background activity is restricted'), findsOneWidget);
    expect(find.text('Battery optimization is on'), findsNothing);
    expect(actions, isEmpty);
  });

  testWidgets('failed status read clears old assurance and offers retry', (
    tester,
  ) async {
    status['background'] = 'unrestricted';
    await openSettings(tester);
    expect(
      find.text('No Android battery restriction detected'),
      findsOneWidget,
    );
    fail = true;
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pumpAndSettle();
    expect(find.text('No Android battery restriction detected'), findsNothing);
    expect(find.text('Could not check settings'), findsOneWidget);
    fail = false;
    await tester.tap(find.text('Try again'));
    await tester.pumpAndSettle();
    expect(
      find.text('No Android battery restriction detected'),
      findsOneWidget,
    );
  });

  testWidgets('system settings failure gives a manual route', (tester) async {
    status['nearby'] = 'permanentlyDenied';
    await openSettings(tester);
    fail = true;
    await tester.tap(find.byKey(const Key('nearbyPermissionButton')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Settings → Apps → Repose'), findsOneWidget);
  });

  testWidgets('iOS explains background limits without Android battery advice', (
    tester,
  ) async {
    status = {
      'platform': 'ios',
      'nearby': 'notRequired',
      'notifications': 'notRequired',
      'background': 'restricted',
      'powerSaver': true,
    };
    await openSettings(tester);
    expect(
      find.textContaining('iOS manages background activity'),
      findsOneWidget,
    );
    expect(find.textContaining('realme'), findsNothing);
    expect(find.byKey(const Key('backgroundSettingsButton')), findsNothing);
  });

  testWidgets('settings and rationale fit narrow phones at 200 percent text', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(360, 800));
    tester.platformDispatcher.textScaleFactorTestValue = 2;
    addTearDown(() async {
      tester.platformDispatcher.clearTextScaleFactorTestValue();
      await tester.binding.setSurfaceSize(null);
    });
    await openSettings(tester);
    expect(tester.takeException(), isNull);
    final action = find.byKey(const Key('backgroundSettingsButton'));
    await tester.ensureVisible(action);
    await tester.tap(action);
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    final cancel = find.text('Not now');
    await tester.ensureVisible(cancel);
    await tester.tap(cancel);
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });
}
