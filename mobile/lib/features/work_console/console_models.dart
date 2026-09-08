String _string(Map<String, dynamic> json, String key, {int max = 256}) {
  final value = json[key];
  if (value is! String || value.isEmpty || value.length > max) {
    throw const FormatException('Invalid console data');
  }
  return value;
}

List<T> _list<T>(
  dynamic value,
  int max,
  T Function(Map<String, dynamic>) parse,
) {
  if (value is! List || value.length > max) {
    throw const FormatException('Invalid console data');
  }
  return value
      .map((item) {
        if (item is! Map<String, dynamic>) {
          throw const FormatException('Invalid console data');
        }
        return parse(item);
      })
      .toList(growable: false);
}

class ConsoleAction {
  ConsoleAction.fromJson(Map<String, dynamic> json)
    : id = _string(json, 'id'),
      name = _string(json, 'name'),
      icon = _string(json, 'icon'),
      kind = _string(json, 'kind'),
      stepCount = (json['steps'] as List).length {
    if (kind != 'hotkey' && kind != 'sequence' ||
        stepCount < 1 ||
        stepCount > 20) {
      throw const FormatException('Invalid console action');
    }
  }
  final String id, name, icon, kind;
  final int stepCount;
}

class ConsoleApp {
  ConsoleApp.fromJson(Map<String, dynamic> json)
    : id = _string(json, 'id'),
      name = _string(json, 'name'),
      actions = _list(json['actions'], 96, ConsoleAction.fromJson) {
    if (actions.map((a) => a.id).toSet().length != actions.length) {
      throw const FormatException('Duplicate action');
    }
  }
  final String id, name;
  final List<ConsoleAction> actions;
}

class ConsoleStatus {
  ConsoleStatus.fromJson(Map<String, dynamic> json)
    : revision = (json['config'] as Map)['revision'] as int,
      apps = _list((json['config'] as Map)['apps'], 16, ConsoleApp.fromJson),
      enabled = json['enabled'] as bool,
      connected = json['connected'] as bool,
      running = json['running'] as bool,
      blocked = json['blocked'] as bool,
      accessibility = json['accessibility'] as bool,
      activeAppId = json['activeAppId'] as String?,
      lastError = json['lastError'] as String? {
    if (revision < 0 || apps.map((a) => a.id).toSet().length != apps.length) {
      throw const FormatException('Invalid console config');
    }
  }
  final int revision;
  final List<ConsoleApp> apps;
  final bool enabled, connected, running, blocked, accessibility;
  final String? activeAppId, lastError;
}
