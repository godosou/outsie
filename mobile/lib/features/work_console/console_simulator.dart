import 'dart:async';
import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'console_client.dart';
import 'console_models.dart';

/// This build flag is only honored in Debug. Production always uses native BLE.
const consoleSimulationEnabled =
    kDebugMode && bool.fromEnvironment('REPOSE_CONSOLE_SIMULATION');

class SimulatedConsoleClient implements ConsoleTransport {
  SimulatedConsoleClient(String deviceId) {
    if (deviceId != device.id) throw ArgumentError('Unknown simulated Mac');
  }
  static const device = ConsoleDevice(
    id: 'simulated-mac',
    name: '模拟 Mac · 蓝牙仿真',
  );
  static Future<List<ConsoleDevice>> devices() async => [device];
  final Map<String, dynamic> _config = {
    'revision': 0,
    'apps': [
      {
        'id': 'tmux',
        'name': 'tmux',
        'bundleId': 'com.apple.Terminal',
        'actions': [
          {
            'id': 'split',
            'name': '分屏 · 等待 2 秒',
            'icon': '◫',
            'kind': 'sequence',
            'steps': [
              {
                'key': 'b',
                'modifiers': ['ctrl'],
                'delayMs': 0,
              },
              {'key': '%', 'modifiers': <String>[], 'delayMs': 2000},
            ],
          },
          {
            'id': 'zoom',
            'name': '放大 / 还原',
            'icon': '⛶',
            'kind': 'sequence',
            'steps': [
              {
                'key': 'b',
                'modifiers': ['ctrl'],
                'delayMs': 0,
              },
              {'key': 'z', 'modifiers': <String>[], 'delayMs': 100},
            ],
          },
        ],
      },
      {
        'id': 'codex',
        'name': 'Codex',
        'bundleId': 'com.openai.codex',
        'actions': [
          {
            'id': 'new-task',
            'name': '新建任务',
            'icon': '+',
            'kind': 'hotkey',
            'steps': [
              {
                'key': 'n',
                'modifiers': ['meta'],
                'delayMs': 0,
              },
            ],
          },
        ],
      },
      {
        'id': 'feishu',
        'name': '飞书',
        'bundleId': 'com.electron.lark',
        'actions': [
          {
            'id': 'search',
            'name': '搜索',
            'icon': '⌕',
            'kind': 'hotkey',
            'steps': [
              {
                'key': 'k',
                'modifiers': ['meta'],
                'delayMs': 0,
              },
            ],
          },
        ],
      },
    ],
  };
  final List<String> executionTrace = [];
  final Set<String> _requests = {};
  Timer? _timer;
  bool _closed = false, _running = false;
  String _activeApp = 'tmux';

  Map<String, dynamic> _app(dynamic id) =>
      (_config['apps'] as List).cast<Map<String, dynamic>>().firstWhere(
        (app) => app['id'] == id,
        orElse: () => throw ConsoleRequestRejected('Unknown simulated App'),
      );

  @override
  Future<ConsoleStatus> request(Map<String, dynamic> message) async {
    if (_closed) throw const ConsoleDisconnected();
    final type = message['type'];
    if (type != 'status') {
      if (message['requestId'] is! String ||
          !_requests.add(message['requestId'] as String)) {
        throw ConsoleRequestRejected('Duplicate simulation request');
      }
    }
    switch (type) {
      case 'status':
        break;
      case 'cancel':
        _timer?.cancel();
        _running = false;
      case 'activate':
        if (_running) throw ConsoleRequestRejected('Simulation is running');
        _activeApp = _app(message['appId'])['id'] as String;
      case 'execute':
        if (_running) throw ConsoleRequestRejected('Simulation is running');
        final app = _app(message['appId']);
        final action = (app['actions'] as List)
            .cast<Map<String, dynamic>>()
            .firstWhere(
              (action) => action['id'] == message['actionId'],
              orElse: () =>
                  throw ConsoleRequestRejected('Unknown simulated action'),
            );
        _activeApp = app['id'] as String;
        _running = true;
        _schedule((action['steps'] as List).cast<Map<String, dynamic>>(), 0);
      case 'reorder':
        final app = _app(message['appId']);
        final ids = message['actionIds'];
        final actions = (app['actions'] as List).cast<Map<String, dynamic>>();
        if (message['revision'] != _config['revision'] ||
            ids is! List ||
            ids.length != actions.length ||
            ids.toSet().length != ids.length ||
            ids.any((id) => !actions.any((action) => action['id'] == id))) {
          throw ConsoleRequestRejected('Layout revision conflict');
        }
        app['actions'] = ids
            .map((id) => actions.firstWhere((action) => action['id'] == id))
            .toList();
        _config['revision'] = (_config['revision'] as int) + 1;
      default:
        throw ConsoleRequestRejected('Unknown simulation request');
    }
    // Round-trip isolates each response from subsequent in-memory mutations.
    return ConsoleStatus.fromJson(
      jsonDecode(
            jsonEncode({
              'config': _config,
              'enabled': true,
              'connected': true,
              'running': _running,
              'blocked': false,
              'accessibility': true,
              'activeAppId': _activeApp,
              'lastError': null,
              'transport': 'bluetooth',
            }),
          )
          as Map<String, dynamic>,
    );
  }

  void _schedule(List<Map<String, dynamic>> steps, int index) {
    if (_closed || !_running) return;
    if (index >= steps.length) {
      _running = false;
      return;
    }
    final step = steps[index];
    _timer = Timer(Duration(milliseconds: step['delayMs'] as int), () {
      if (_closed || !_running) return;
      executionTrace.add(step['key'] as String);
      _schedule(steps, index + 1);
    });
  }

  @override
  void close() {
    _closed = true;
    _running = false;
    _timer?.cancel();
  }
}
