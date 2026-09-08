import 'dart:async';
import 'dart:convert';
import 'package:flutter/services.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/app/repose_theme.dart';
import 'package:repose_unlock/features/work_console/console_client.dart';
import 'package:repose_unlock/features/work_console/console_controller.dart';
import 'package:repose_unlock/features/work_console/console_models.dart';
import 'package:repose_unlock/features/work_console/work_console_page.dart';
import 'package:repose_unlock/features/work_console/console_simulator.dart';

Map<String, dynamic> statusJson({
  bool blocked = false,
  bool running = false,
  int revision = 1,
}) => {
  'enabled': true,
  'connected': true,
  'accessibility': true,
  'blocked': blocked,
  'running': running,
  'activeAppId': 'tmux',
  'lastError': null,
  'config': {
    'revision': revision,
    'apps': [
      {
        'id': 'tmux',
        'name': 'tmux',
        'bundleId': 'com.apple.Terminal',
        'actions': [
          {
            'id': 'split',
            'name': 'Split pane',
            'icon': 'split',
            'kind': 'hotkey',
            'steps': [
              {
                'key': 'd',
                'modifiers': ['meta'],
                'delayMs': 0,
              },
            ],
          },
          {
            'id': 'sequence',
            'name': 'Open workspace',
            'icon': 'play',
            'kind': 'sequence',
            'steps': [
              {
                'key': 'b',
                'modifiers': ['ctrl'],
                'delayMs': 0,
              },
              {'key': '%', 'modifiers': [], 'delayMs': 50},
            ],
          },
        ],
      },
    ],
  },
};
const macId = 'paired-mac';

class FakeTransport implements ConsoleTransport {
  final messages = <Map<String, dynamic>>[];
  bool closed = false, fail = false, reject = false;
  Map<String, dynamic> value = statusJson();
  Completer<ConsoleStatus>? pending;
  @override
  Future<ConsoleStatus> request(Map<String, dynamic> message) async {
    messages.add(message);
    if (fail) throw const ConsoleDisconnected();
    if (reject) throw ConsoleRequestRejected();
    return pending?.future ?? ConsoleStatus.fromJson(value);
  }

  @override
  void close() {
    closed = true;
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = ConsoleClient.bluetoothChannel;
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  tearDown(() => messenger.setMockMethodCallHandler(channel, null));
  test(
    'all actions reorder with revision and arrangement suppresses execution',
    () async {
      final transport = FakeTransport();
      final controller = ConsoleController(
        transportFactory: (_) => transport,
        devicesLoader: () async => [
          const ConsoleDevice(id: macId, name: 'Paired Mac'),
        ],
      );
      await controller.connect(macId);
      controller.beginArrange();
      controller.move('sequence', 0);
      await controller.execute('split');
      expect(transport.messages.length, 1);
      await controller.saveOrder();
      expect(transport.messages.last['type'], 'reorder');
      expect(transport.messages.last['revision'], 1);
      expect(transport.messages.last['actionIds'], ['sequence', 'split']);
      expect(controller.arranging, isFalse);
      controller.dispose();
      expect(transport.closed, isTrue);
    },
  );
  test(
    'blocked state prevents execution and disconnect does not retry',
    () async {
      final transport = FakeTransport()..value = statusJson(blocked: true);
      final controller = ConsoleController(
        transportFactory: (_) => transport,
        devicesLoader: () async => [
          const ConsoleDevice(id: macId, name: 'Paired Mac'),
        ],
      );
      await controller.connect(macId);
      await controller.execute('split');
      expect(transport.messages.length, 1);
      transport.value = statusJson();
      await controller.connect(macId);
      transport.fail = true;
      await controller.execute('split');
      final count = transport.messages.length;
      await controller.execute('split');
      expect(transport.messages.length, count);
      expect(controller.connected, isFalse);
      controller.dispose();
    },
  );
  test(
    'late response cannot restore disconnected session; no overlapping requests',
    () async {
      final transport = FakeTransport();
      final controller = ConsoleController(
        transportFactory: (_) => transport,
        devicesLoader: () async => [
          const ConsoleDevice(id: macId, name: 'Paired Mac'),
        ],
      );
      await controller.connect(macId);
      transport.pending = Completer();
      final first = controller.execute('split');
      await controller.execute('sequence');
      expect(transport.messages.length, 2);
      controller.disconnect();
      transport.pending!.complete(ConsoleStatus.fromJson(statusJson()));
      await first;
      expect(controller.connected, isFalse);
      controller.dispose();
    },
  );
  test(
    'revision rejection preserves draft and keeps connection available',
    () async {
      final transport = FakeTransport();
      final controller = ConsoleController(
        transportFactory: (_) => transport,
        devicesLoader: () async => [
          const ConsoleDevice(id: macId, name: 'Paired Mac'),
        ],
      );
      await controller.connect(macId);
      controller.beginArrange();
      transport.reject = true;
      await controller.saveOrder();
      expect(controller.arranging, isTrue);
      expect(controller.connected, isTrue);
      expect(controller.error, isNotNull);
      controller.dispose();
    },
  );
  testWidgets(
    'sequence and shortcut buttons can move; arrange mode never executes',
    (tester) async {
      final transport = FakeTransport();
      final controller = ConsoleController(
        transportFactory: (_) => transport,
        devicesLoader: () async => [
          const ConsoleDevice(id: macId, name: 'Paired Mac'),
        ],
      );
      await controller.connect(macId);
      await tester.pumpWidget(
        MaterialApp(
          theme: reposeTheme(Brightness.light),
          home: WorkConsolePage(controller: controller),
        ),
      );
      expect(find.text('Split pane'), findsOneWidget);
      await tester.tap(find.text('Arrange'));
      await tester.pump();
      expect(find.byType(LongPressDraggable<String>), findsNWidgets(2));
      final tiles = find.byType(LongPressDraggable<String>);
      final gesture = await tester.startGesture(tester.getCenter(tiles.first));
      await tester.pump(const Duration(milliseconds: 600));
      await gesture.moveTo(tester.getCenter(tiles.last));
      await tester.pump();
      await gesture.up();
      await tester.pump();
      expect(controller.draftOrder, ['sequence', 'split']);
      await tester.tap(find.byTooltip('Move earlier').last);
      await tester.pump();
      expect(controller.draftOrder, ['split', 'sequence']);
      await tester.tap(find.byTooltip('Move earlier').last);
      await tester.pump();
      expect(controller.draftOrder, ['sequence', 'split']);
      await tester.tap(find.byKey(const ValueKey('console-action-sequence')));
      await tester.pump();
      expect(transport.messages.length, 1);
      await tester.tap(find.text('Save layout'));
      await tester.pump();
      expect(transport.messages.last['type'], 'reorder');
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      controller.dispose();
    },
  );

  test(
    'BLE lists confirmed devices and connects by native association without network credentials',
    () async {
      final calls = <MethodCall>[];
      final messages = <Map<String, dynamic>>[];
      messenger.setMockMethodCallHandler(channel, (call) async {
        calls.add(call);
        switch (call.method) {
          case 'devices':
            return [
              {'id': macId, 'name': 'Paired Mac'},
            ];
          case 'connect':
            return null;
          case 'request':
            messages.add(
              jsonDecode((call.arguments as Map)['message'] as String)
                  as Map<String, dynamic>,
            );
            final value = statusJson();
            if (messages.length > 1) value.remove('config');
            return jsonEncode({'ok': true, 'data': value});
          case 'disconnect':
            return null;
        }
        throw StateError('Unexpected channel method');
      });
      expect((await ConsoleClient.devices()).single.id, macId);
      final client = ConsoleClient(macId);
      expect((await client.request({'type': 'status'})).apps.first.id, 'tmux');
      expect(
        (await client.request({'type': 'status'})).apps.first.actions.length,
        2,
      );
      expect(calls.firstWhere((call) => call.method == 'connect').arguments, {
        'deviceId': macId,
      });
      expect(messages, [
        {'type': 'status'},
        {'type': 'status', 'knownRevision': 1},
      ]);
      client.close();
      await Future<void>.delayed(Duration.zero);
      expect(calls.last.method, 'disconnect');
    },
  );

  test(
    'BLE rejects missing initial config, never replays failed actions and preserves IDs',
    () async {
      final messages = <Map<String, dynamic>>[];
      messenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method != 'request') return null;
        final message =
            jsonDecode((call.arguments as Map)['message'] as String)
                as Map<String, dynamic>;
        messages.add(message);
        if (message['type'] == 'execute') {
          throw PlatformException(
            code: 'disconnected',
            message: 'GATT disconnected',
          );
        }
        return jsonEncode({'ok': true, 'data': statusJson()});
      });
      final client = ConsoleClient(macId);
      await client.request({'type': 'status'});
      await expectLater(
        client.request({
          'type': 'execute',
          'requestId': 'once',
          'appId': 'tmux',
          'actionId': 'split',
        }),
        throwsA(isA<PlatformException>()),
      );
      await expectLater(
        client.request({
          'type': 'execute',
          'requestId': 'once',
          'appId': 'tmux',
          'actionId': 'split',
        }),
        throwsA(isA<ConsoleDisconnected>()),
      );
      expect(messages.last, {
        'type': 'execute',
        'requestId': 'once',
        'appId': 'tmux',
        'actionId': 'split',
        'knownRevision': 1,
      });
      expect(messages.length, 2);
      messenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method != 'request') return null;
        return jsonEncode({'ok': true, 'data': statusJson()..remove('config')});
      });
      final empty = ConsoleClient(macId);
      await expectLater(
        empty.request({'type': 'status'}),
        throwsFormatException,
      );
      empty.close();
    },
  );

  test(
    'BLE cache refreshes on new revision and rejection preserves connection',
    () async {
      var revision = 1;
      messenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method != 'request') return null;
        final message =
            jsonDecode((call.arguments as Map)['message'] as String) as Map;
        if (message['type'] == 'reorder') {
          return jsonEncode({'ok': false, 'error': '配置已更新'});
        }
        final value = statusJson(revision: revision);
        if (message['knownRevision'] == revision) value.remove('config');
        return jsonEncode({'ok': true, 'data': value});
      });
      final client = ConsoleClient(macId);
      await client.request({'type': 'status'});
      revision = 2;
      expect((await client.request({'type': 'status'})).revision, 2);
      await expectLater(
        client.request({
          'type': 'reorder',
          'requestId': 'stale',
          'appId': 'tmux',
          'revision': 1,
          'actionIds': ['sequence', 'split'],
        }),
        throwsA(isA<ConsoleRequestRejected>()),
      );
      expect((await client.request({'type': 'status'})).revision, 2);
      client.close();
    },
  );

  test(
    'pending BLE result cannot survive disconnect or disconnect a replacement session',
    () async {
      final reply = Completer<String>();
      final pendingStarted = Completer<void>();
      var requests = 0, disconnects = 0;
      messenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method == 'disconnect') {
          disconnects++;
          return null;
        }
        if (call.method != 'request') return null;
        requests++;
        if (requests == 1) {
          pendingStarted.complete();
          return reply.future;
        }
        return jsonEncode({'ok': true, 'data': statusJson()});
      });
      final old = ConsoleClient(macId);
      final pending = old.request({'type': 'status'});
      final rejected = expectLater(
        pending,
        throwsA(isA<ConsoleDisconnected>()),
      );
      await pendingStarted.future;
      final replacement = ConsoleClient(macId);
      await replacement.request({'type': 'status'});
      old.close();
      reply.complete(jsonEncode({'ok': true, 'data': statusJson()}));
      await rejected;
      expect(disconnects, 0);
      expect((await replacement.request({'type': 'status'})).enabled, isTrue);
      replacement.close();
      await Future<void>.delayed(Duration.zero);
      expect(disconnects, 1);
    },
  );

  testWidgets(
    'empty paired list returns to existing Phone Key flow and offers no separate QR',
    (tester) async {
      final controller = ConsoleController(devicesLoader: () async => []);
      await tester.pumpWidget(
        MaterialApp(
          home: Builder(
            builder: (context) => Scaffold(
              body: TextButton(
                onPressed: () => Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => WorkConsolePage(controller: controller),
                  ),
                ),
                child: const Text('Phone Key home'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Phone Key home'));
      await tester.pumpAndSettle();
      expect(find.text('Go to Phone Key pairing'), findsOneWidget);
      expect(find.textContaining('Wi-Fi'), findsNothing);
      expect(find.text('Scan Mac QR code'), findsNothing);
      await tester.tap(find.text('Go to Phone Key pairing'));
      await tester.pumpAndSettle();
      expect(find.text('Phone Key home'), findsOneWidget);
      controller.dispose();
    },
  );

  testWidgets(
    'explicit simulation models a two-second sequence and cancellation with no native channel',
    (tester) async {
      var nativeCalls = 0;
      messenger.setMockMethodCallHandler(channel, (call) async {
        nativeCalls++;
        throw StateError('Simulation must not call native');
      });
      final simulator = SimulatedConsoleClient(
        SimulatedConsoleClient.device.id,
      );
      final initial = await simulator.request({'type': 'status'});
      expect(initial.apps.map((app) => app.id), ['tmux', 'codex', 'feishu']);
      await simulator.request({
        'type': 'execute',
        'requestId': 'run1',
        'appId': 'tmux',
        'actionId': 'split',
      });
      await tester.pump(const Duration(milliseconds: 1));
      expect(simulator.executionTrace, ['b']);
      await tester.pump(const Duration(seconds: 1));
      expect((await simulator.request({'type': 'status'})).running, isTrue);
      await simulator.request({'type': 'cancel', 'requestId': 'stop1'});
      await tester.pump(const Duration(seconds: 2));
      expect(simulator.executionTrace, ['b']);
      expect((await simulator.request({'type': 'status'})).running, isFalse);
      await simulator.request({
        'type': 'execute',
        'requestId': 'run2',
        'appId': 'tmux',
        'actionId': 'split',
      });
      await tester.pump(const Duration(milliseconds: 1));
      await tester.pump(const Duration(seconds: 2));
      expect(simulator.executionTrace, ['b', 'b', '%']);
      simulator.close();
      expect(nativeCalls, 0);
    },
  );

  testWidgets(
    'simulation is visibly labeled and can connect using the same control page',
    (tester) async {
      final controller = ConsoleController(simulation: true);
      await tester.pumpWidget(
        MaterialApp(home: WorkConsolePage(controller: controller)),
      );
      await tester.pump();
      expect(find.textContaining('BLUETOOTH SIMULATION'), findsOneWidget);
      await tester.tap(find.text(SimulatedConsoleClient.device.name));
      await tester.pump();
      expect(controller.connected, isTrue);
      expect(find.text('tmux'), findsWidgets);
      await tester.pumpWidget(const SizedBox());
      controller.dispose();
    },
  );
}
