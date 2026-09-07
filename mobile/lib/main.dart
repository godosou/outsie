import 'package:flutter/material.dart';

import 'app/repose_unlock_app.dart';
import 'native/native_gateway.dart';

void main() {
  runApp(const ReposeUnlockApp(gateway: UnavailableNativeGateway()));
}
