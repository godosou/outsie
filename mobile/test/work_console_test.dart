import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:crypto/crypto.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/features/work_console/console_client.dart';
import 'package:repose_unlock/features/work_console/console_controller.dart';
import 'package:repose_unlock/features/work_console/console_models.dart';
import 'package:repose_unlock/features/work_console/work_console_page.dart';

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
String qr({String host = '127.0.0.1', int port = 1234, String? fingerprint}) =>
    '${ConsolePairing.prefix}${base64UrlEncode(utf8.encode(jsonEncode({'v': 1, 'host': host, 'port': port, 'token': 'a' * 43, 'fingerprint': fingerprint ?? 'a' * 64}))).replaceAll('=', '')}';

class RealHttpOverrides extends HttpOverrides {}

class FakeTransport implements ConsoleTransport {
  final messages = <Map<String, dynamic>>[];
  bool closed = false, fail = false, reject = false;
  Map<String, dynamic> value = statusJson();
  Completer<ConsoleStatus>? pending;
  @override
  Future<ConsoleStatus> request(Map<String, dynamic> message) async {
    messages.add(message);
    if (fail) throw const SocketException('offline');
    if (reject) throw ConsoleRequestRejected();
    return pending?.future ?? ConsoleStatus.fromJson(value);
  }

  @override
  void close() {
    closed = true;
  }
}

void main() {
  test(
    'QR parser accepts IP and rejects alternate URI, hosts and credentials',
    () {
      expect(ConsolePairing.parse(qr()).port, 1234);
      expect(
        () => ConsolePairing.parse(qr(host: 'example.com')),
        throwsFormatException,
      );
      expect(() => ConsolePairing.parse(qr(port: 0)), throwsFormatException);
      expect(
        () => ConsolePairing.parse(qr(fingerprint: 'bad')),
        throwsFormatException,
      );
      expect(
        () => ConsolePairing.parse('repose://pair/v1/abc'),
        throwsFormatException,
      );
    },
  );
  test('certificate fingerprint must match the exact DER', () {
    final der = [1, 2, 3];
    expect(
      matchesConsoleCertificate(der, sha256.convert(der).toString()),
      isTrue,
    );
    expect(
      matchesConsoleCertificate([1, 2, 4], sha256.convert(der).toString()),
      isFalse,
    );
  });
  test(
    'all actions reorder with revision and arrangement suppresses execution',
    () async {
      final transport = FakeTransport();
      final controller = ConsoleController(transportFactory: (_) => transport);
      await controller.connect(qr());
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
      final controller = ConsoleController(transportFactory: (_) => transport);
      await controller.connect(qr());
      await controller.execute('split');
      expect(transport.messages.length, 1);
      transport.value = statusJson();
      await controller.connect(qr());
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
      final controller = ConsoleController(transportFactory: (_) => transport);
      await controller.connect(qr());
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
      final controller = ConsoleController(transportFactory: (_) => transport);
      await controller.connect(qr());
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
      final controller = ConsoleController(transportFactory: (_) => transport);
      await controller.connect(qr());
      await tester.pumpWidget(
        MaterialApp(home: WorkConsolePage(controller: controller)),
      );
      expect(find.text('Split pane'), findsOneWidget);
      await tester.tap(find.text('Arrange'));
      await tester.pump();
      expect(find.byType(LongPressDraggable<String>), findsNWidgets(2));
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
    'TLS pin rejects a different certificate before sending any credential',
    () => HttpOverrides.runWithHttpOverrides(() async {
      final context = SecurityContext()
        ..useCertificateChain('test/fixtures/console_tls/cert.pem')
        ..usePrivateKey('test/fixtures/console_tls/key.pem');
      final server = await HttpServer.bindSecure(
        InternetAddress.loopbackIPv4,
        0,
        context,
      );
      var requests = 0;
      final subscription = server.listen((request) {
        requests++;
        request.response.close();
      }, onError: (_) {});
      final client = ConsoleClient(ConsolePairing.parse(qr(port: server.port)));
      await expectLater(
        client.request({'type': 'status'}),
        throwsA(isA<HandshakeException>()),
      );
      expect(requests, 0);
      client.close();
      await subscription.cancel();
      await server.close(force: true);
    }, RealHttpOverrides()),
  );
  test(
    'TLS pinned request works; failed execution is sent once and client closes',
    () => HttpOverrides.runWithHttpOverrides(() async {
      final pem = File('test/fixtures/console_tls/cert.pem').readAsStringSync();
      final der = base64Decode(
        pem.replaceAll(RegExp(r'-----[^-]+-----|\s'), ''),
      );
      final context = SecurityContext()
        ..useCertificateChain('test/fixtures/console_tls/cert.pem')
        ..usePrivateKey('test/fixtures/console_tls/key.pem');
      final server = await HttpServer.bindSecure(
        InternetAddress.loopbackIPv4,
        0,
        context,
      );
      final messages = <String>[];
      final subscription = server.listen((request) async {
        expect(request.contentLength, greaterThan(0));
        expect(request.headers.value('authorization'), 'Bearer ${'a' * 43}');
        final message =
            jsonDecode(await utf8.decoder.bind(request).join()) as Map;
        messages.add(message['type'] as String);
        request.response.headers.contentType = ContentType.json;
        if (message['type'] == 'reorder') {
          request.response.statusCode = 400;
          request.response.write(
            jsonEncode({'ok': false, 'error': 'revision conflict'}),
          );
          await request.response.close();
          return;
        }
        request.response.write(
          message['type'] == 'status'
              ? jsonEncode({'ok': true, 'data': statusJson()})
              : 'malformed',
        );
        await request.response.close();
      });
      final client = ConsoleClient(
        ConsolePairing.parse(
          qr(port: server.port, fingerprint: sha256.convert(der).toString()),
        ),
      );
      expect((await client.request({'type': 'status'})).apps.first.id, 'tmux');
      await expectLater(
        client.request({'type': 'reorder', 'requestId': 'conflict'}),
        throwsA(isA<ConsoleRequestRejected>()),
      );
      expect((await client.request({'type': 'status'})).enabled, isTrue);
      await expectLater(
        client.request({'type': 'execute', 'requestId': 'test'}),
        throwsFormatException,
      );
      await expectLater(
        client.request({'type': 'execute', 'requestId': 'test'}),
        throwsA(isA<SocketException>()),
      );
      expect(messages, ['status', 'reorder', 'status', 'execute']);
      client.close();
      await subscription.cancel();
      await server.close(force: true);
    }, RealHttpOverrides()),
  );
}
