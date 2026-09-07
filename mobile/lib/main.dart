import 'package:flutter/material.dart';

import 'app/repose_unlock_app.dart';
import 'native/pigeon_native_gateway.dart';

void main() {
  runApp(ReposeUnlockApp(gateway: createNativeGateway()));
}
