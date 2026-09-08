import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'app_text.dart';
import 'repose_theme.dart';
import '../features/settings/settings_page.dart';

import '../features/calibration/calibration_controller.dart';
import '../features/devices/device_controller.dart';
import '../features/pairing/pairing_controller.dart';
import '../features/pairing/pairing_scanner.dart';
import '../native/native_gateway.dart';
import '../native/native_models.dart';

class ReposeUnlockApp extends StatelessWidget {
  const ReposeUnlockApp({
    required this.gateway,
    this.pairingCameraAccess = const SystemPairingCameraAccess(),
    this.pairingScannerBuilder,
    super.key,
  });

  final NativeGateway gateway;
  final PairingCameraAccess pairingCameraAccess;
  final PairingScannerBuilder? pairingScannerBuilder;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'Repose Phone Key',
      theme: reposeTheme(Brightness.light),
      darkTheme: reposeTheme(Brightness.dark),
      themeMode: ThemeMode.system,
      supportedLocales: const [Locale('en'), Locale('zh')],
      localizationsDelegates: GlobalMaterialLocalizations.delegates,
      home: _CompanionHome(
        gateway: gateway,
        pairingCameraAccess: pairingCameraAccess,
        pairingScannerBuilder: pairingScannerBuilder,
      ),
    );
  }
}

class _CompanionHome extends StatefulWidget {
  const _CompanionHome({
    required this.gateway,
    required this.pairingCameraAccess,
    this.pairingScannerBuilder,
  });

  final NativeGateway gateway;
  final PairingCameraAccess pairingCameraAccess;
  final PairingScannerBuilder? pairingScannerBuilder;

  @override
  State<_CompanionHome> createState() => _CompanionHomeState();
}

class _CompanionHomeState extends State<_CompanionHome> {
  late final DeviceController _devices;
  late final PairingController _pairing;
  late final CalibrationController _calibration;
  late final Listenable _controllers;
  var _showAdditionalPairing = false;
  var _openingScanner = false;

  @override
  void initState() {
    super.initState();
    _devices = DeviceController(gateway: widget.gateway);
    _pairing = PairingController(
      gateway: widget.gateway,
      deviceController: _devices,
    );
    _calibration = CalibrationController(
      gateway: widget.gateway,
      deviceController: _devices,
    );
    _controllers = Listenable.merge(<Listenable>[
      _devices,
      _pairing,
      _calibration,
    ]);
    _devices.addListener(_syncCalibrationFromAuthoritativeSnapshot);
    unawaited(_hydrateFromNative());
  }

  Future<void> _hydrateFromNative() async {
    final hydrated = await _devices.hydrate();
    if (!hydrated || !mounted) {
      return;
    }
    final snapshot = _devices.state.snapshot;
    if (snapshot == null) {
      return;
    }
    _pairing.hydrateFromSnapshot(snapshot.pendingPairing);
  }

  void _syncCalibrationFromAuthoritativeSnapshot() {
    final deviceState = _devices.state;
    final snapshot = deviceState.snapshot;
    if (!deviceState.isHydrated ||
        deviceState.isMutationBusy ||
        snapshot == null) {
      return;
    }
    _calibration.hydrateFromSnapshot(snapshot.calibration);
  }

  @override
  void dispose() {
    _devices.removeListener(_syncCalibrationFromAuthoritativeSnapshot);
    _calibration.dispose();
    _pairing.dispose();
    _devices.dispose();
    super.dispose();
  }

  Future<void> _openSettings() async {
    await Navigator.of(
      context,
    ).push(MaterialPageRoute<void>(builder: (_) => const SettingsPage()));
    if (mounted && !_pairing.state.isBusy && !_calibration.state.isBusy) {
      await _hydrateFromNative();
    }
  }

  Future<void> _scanPairingQr() async {
    if (_openingScanner || !_devices.state.gate.canPair) {
      return;
    }
    setState(() => _openingScanner = true);
    var retryAfterDenial = false;
    try {
      final shouldContinue = await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          key: const Key('cameraPermissionRationaleDialog'),
          icon: const Icon(Icons.qr_code_scanner_rounded),
          title: const AppText('Use camera to pair?'),
          content: const AppText(
            'Repose uses the camera only while this scanner is open. It reads the one-time QR code on your Mac and does not save photos.',
          ),
          actions: <Widget>[
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const AppText('Not now'),
            ),
            FilledButton(
              key: const Key('continueToCameraButton'),
              onPressed: () => Navigator.of(context).pop(true),
              child: const AppText('Use camera'),
            ),
          ],
        ),
      );
      if (shouldContinue != true || !mounted) {
        return;
      }

      final access = await widget.pairingCameraAccess.request();
      if (!mounted) {
        return;
      }
      switch (access) {
        case CameraAccessOutcome.granted:
          final payload = await Navigator.of(context).push<String>(
            MaterialPageRoute<String>(
              builder: (_) => PairingScannerPage(
                scannerBuilder: widget.pairingScannerBuilder,
              ),
            ),
          );
          if (payload != null && mounted) {
            await _pairing.beginPairing(payload);
          }
        case CameraAccessOutcome.denied:
          retryAfterDenial = await _showCameraDeniedDialog();
        case CameraAccessOutcome.permanentlyDenied:
          await _showCameraPermanentlyDeniedDialog();
      }
    } finally {
      if (mounted) {
        setState(() => _openingScanner = false);
      }
    }
    if (retryAfterDenial && mounted) {
      unawaited(_scanPairingQr());
    }
  }

  Future<bool> _showCameraDeniedDialog() async {
    if (!mounted) {
      return false;
    }
    return await showDialog<bool>(
          context: context,
          builder: (context) => AlertDialog(
            key: const Key('cameraPermissionDeniedDialog'),
            icon: const Icon(Icons.no_photography_outlined),
            title: const AppText('Camera access was not allowed'),
            content: const AppText(
              'The QR scanner stays closed. You can try again or keep using your Mac password.',
            ),
            actions: <Widget>[
              TextButton(
                onPressed: () => Navigator.of(context).pop(false),
                child: const AppText('Not now'),
              ),
              FilledButton(
                onPressed: () => Navigator.of(context).pop(true),
                child: const AppText('Try again'),
              ),
            ],
          ),
        ) ??
        false;
  }

  Future<void> _showCameraPermanentlyDeniedDialog() async {
    if (!mounted) {
      return;
    }
    final openSettings = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        key: const Key('cameraPermissionPermanentlyDeniedDialog'),
        icon: const Icon(Icons.settings_outlined),
        title: const AppText('Allow camera access in Settings'),
        content: const AppText(
          'Your device will not show the camera prompt again. Open Repose permissions in Settings, allow Camera, then return to scan.',
        ),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const AppText('Not now'),
          ),
          FilledButton(
            key: const Key('openCameraSettingsButton'),
            onPressed: () => Navigator.of(context).pop(true),
            child: const AppText('Open settings'),
          ),
        ],
      ),
    );
    if (openSettings == true) {
      await widget.pairingCameraAccess.openSettings();
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Row(
          children: [
            const ReposeBrandMark(),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const AppText(
                    'repose.',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontWeight: FontWeight.w700,
                      fontSize: 25,
                      letterSpacing: -1,
                    ),
                  ),
                  AppText(
                    'Repose Key',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.labelSmall?.copyWith(
                      color: Theme.of(context).colorScheme.onSurfaceVariant,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
        actions: [
          IconButton(
            tooltip: tr(context, 'Permissions & background'),
            onPressed: _openSettings,
            icon: const Icon(Icons.tune_rounded, size: 22),
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: AnimatedBuilder(
        animation: _controllers,
        builder: (context, _) {
          final deviceState = _devices.state;
          final needsSystemAssociation =
              deviceState.snapshot?.capability ==
              CompanionCapability.associationNotConfigured;
          final canShowWorkflows =
              deviceState.gate.canPair ||
              deviceState.snapshot?.pendingPairing != null ||
              _pairing.state.phase != PairingPhase.idle;
          final showPrimaryPairing =
              deviceState.devices.isEmpty ||
              (!_showAdditionalPairing &&
                  _pairing.state.phase != PairingPhase.idle);
          return SafeArea(
            child: SingleChildScrollView(
              padding: const EdgeInsets.fromLTRB(16, 12, 16, 32),
              child: Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 720),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: <Widget>[
                      _PhoneKeyHero(
                        state: deviceState,
                        calibration: _calibration.state,
                      ),
                      const SizedBox(height: 12),
                      if (needsSystemAssociation) ...[
                        _AssociationCard(controller: _devices),
                        const SizedBox(height: 12),
                      ],
                      if (!deviceState.gate.canPair &&
                          !needsSystemAssociation) ...[
                        FilledButton.icon(
                          onPressed: _openSettings,
                          icon: const Icon(
                            Icons.arrow_forward_rounded,
                            size: 19,
                          ),
                          label: const AppText('See setup guidance'),
                        ),
                        const SizedBox(height: 8),
                        OutlinedButton.icon(
                          onPressed: deviceState.isLoading
                              ? null
                              : () => unawaited(_hydrateFromNative()),
                          icon: const Icon(Icons.refresh_rounded, size: 19),
                          label: const AppText('Check again'),
                        ),
                        const SizedBox(height: 20),
                      ],
                      if (showPrimaryPairing && canShowWorkflows) ...[
                        _PairingCard(
                          gate: deviceState.gate,
                          controller: _pairing,
                          isOpeningScanner: _openingScanner,
                          onScan: _scanPairingQr,
                        ),
                        const SizedBox(height: 12),
                      ],
                      if (deviceState.devices.isNotEmpty) ...[
                        _CalibrationCard(
                          gate: deviceState.gate,
                          hasPairedDevice: true,
                          controller: _calibration,
                        ),
                        const SizedBox(height: 12),
                      ],
                      _SetupProgress(
                        state: deviceState,
                        calibration: _calibration.state,
                      ),
                      const SizedBox(height: 12),
                      if (deviceState.devices.isEmpty && canShowWorkflows) ...[
                        _CalibrationCard(
                          gate: deviceState.gate,
                          hasPairedDevice: false,
                          controller: _calibration,
                        ),
                        const SizedBox(height: 12),
                      ],
                      _DevicesCard(
                        controller: _devices,
                        onAddDevice:
                            deviceState.devices.isNotEmpty &&
                                !_showAdditionalPairing
                            ? () =>
                                  setState(() => _showAdditionalPairing = true)
                            : null,
                      ),
                      if (deviceState.devices.isNotEmpty &&
                          _showAdditionalPairing) ...<Widget>[
                        const SizedBox(height: 12),
                        _PairingCard(
                          gate: deviceState.gate,
                          controller: _pairing,
                          isOpeningScanner: _openingScanner,
                          onScan: _scanPairingQr,
                        ),
                      ],
                      const SizedBox(height: 12),
                      const AppText(
                        'Password sign-in remains available if phone key is '
                        'unavailable.',
                        textAlign: TextAlign.center,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}

class _AssociationCard extends StatelessWidget {
  const _AssociationCard({required this.controller});

  final DeviceController controller;

  @override
  Widget build(BuildContext context) {
    final state = controller.state;
    return _SectionCard(
      icon: Icons.bluetooth_searching_rounded,
      title: 'Connect this phone',
      children: <Widget>[
        const AppText(
          'Let Android find the Repose service advertised by your Mac. This system connection does not unlock your Mac; secure pairing comes next.',
        ),
        const SizedBox(height: 10),
        FilledButton.icon(
          key: const Key('requestCompanionAssociationButton'),
          onPressed: state.isAssociating
              ? null
              : () => unawaited(controller.requestCompanionAssociation()),
          icon: state.isAssociating
              ? const SizedBox.square(
                  dimension: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Icon(Icons.radar_rounded),
          label: const AppText('Find a Repose Mac'),
        ),
        if (state.message != null) ...<Widget>[
          const SizedBox(height: 8),
          _LiveStatus(state.message!),
        ],
      ],
    );
  }
}

class _PhoneKeyHero extends StatelessWidget {
  const _PhoneKeyHero({required this.state, required this.calibration});

  final DeviceState state;
  final CalibrationState calibration;

  @override
  Widget build(BuildContext context) {
    final gate = state.gate;
    final hasPairedDevice = state.devices.isNotEmpty;
    final isCalibrated = calibration.phase == CalibrationPhase.complete;
    final checking =
        state.isLoading ||
        state.snapshot == null ||
        state.snapshot?.capability == CompanionCapability.loading;
    final needsSystemAssociation =
        state.snapshot?.capability ==
        CompanionCapability.associationNotConfigured;
    final (statusLabel, headline, detail) = checking
        ? (
            'CHECKING',
            'Checking your phone key',
            'This will only take a moment.',
          )
        : needsSystemAssociation
        ? (
            'CONNECT YOUR MAC',
            'Connect Android to your Mac',
            'Allow Android to discover the Repose service before secure pairing.',
          )
        : switch ((gate.canPair, hasPairedDevice, isCalibrated)) {
            (false, _, _) => (
              'PHONE KEY OFFLINE',
              'Phone key unavailable',
              gate.message ?? 'Phone key setup is paused.',
            ),
            (true, false, _) => (
              'READY TO PAIR',
              'Add your phone key',
              'Pair this phone with your Mac to continue.',
            ),
            (true, true, false) => (
              'CALIBRATION NEEDED',
              'Set your unlock distance',
              'Choose where your Mac should recognize this phone.',
            ),
            (true, true, true) => (
              'SETUP COMPLETE',
              'Phone key setup complete',
              'Secure setup is complete on this device.',
            ),
          };
    final colors = Theme.of(context).colorScheme;
    final accent = colors.primary;
    final capabilityStatus = gate.canPair
        ? 'Phone key setup is ready'
        : state.message ?? gate.message ?? 'Phone key is unavailable.';

    return Container(
      key: const Key('phoneKeyHero'),
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 18),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(28),
        color: colors.surfaceContainerLow,
        border: Border.all(color: colors.outlineVariant),
      ),
      child: Column(
        children: <Widget>[
          Align(
            alignment: Alignment.centerLeft,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
              decoration: BoxDecoration(
                color: accent.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(999),
                border: Border.all(color: accent.withValues(alpha: 0.38)),
              ),
              child: AppText(
                statusLabel,
                style: Theme.of(context).textTheme.labelSmall?.copyWith(
                  color: accent,
                  fontWeight: FontWeight.w700,
                  letterSpacing: 1.25,
                ),
              ),
            ),
          ),
          const SizedBox(height: 10),
          Container(
            key: const Key('proximityOrb'),
            width: 80,
            height: 80,
            padding: const EdgeInsets.all(14),
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: accent.withValues(alpha: 0.06),
              border: Border.all(color: accent.withValues(alpha: 0.16)),
              boxShadow: <BoxShadow>[
                BoxShadow(
                  color: accent.withValues(alpha: 0.12),
                  blurRadius: 32,
                  spreadRadius: 8,
                ),
              ],
            ),
            child: Container(
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: accent.withValues(alpha: 0.12),
                border: Border.all(color: accent.withValues(alpha: 0.46)),
              ),
              child: Icon(Icons.key_rounded, size: 28, color: accent),
            ),
          ),
          const SizedBox(height: 10),
          Text(
            hasPairedDevice
                ? state.devices.first.displayName
                : tr(context, 'THIS PHONE'),
            style: Theme.of(context).textTheme.labelMedium?.copyWith(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
              fontWeight: FontWeight.w700,
              letterSpacing: 1.1,
            ),
          ),
          const SizedBox(height: 5),
          AppText(
            headline,
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.headlineSmall?.copyWith(
              color: colors.onSurface,
              fontWeight: FontWeight.w700,
            ),
          ),
          const SizedBox(height: 6),
          Semantics(
            container: true,
            liveRegion: true,
            label:
                '${tr(context, 'Phone key status')}\n${tr(context, capabilityStatus)}',
            child: ExcludeSemantics(
              child: Column(
                children: <Widget>[
                  AppText(
                    detail,
                    textAlign: TextAlign.center,
                    style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                      color: Theme.of(context).colorScheme.onSurfaceVariant,
                      height: 1.4,
                    ),
                  ),
                  if (capabilityStatus != detail) ...<Widget>[
                    const SizedBox(height: 10),
                    AppText(
                      capabilityStatus,
                      textAlign: TextAlign.center,
                      style: TextStyle(color: colors.onSurfaceVariant),
                    ),
                  ],
                ],
              ),
            ),
          ),
          if (gate.isPermanentlyUnsupported) ...<Widget>[
            const SizedBox(height: 10),
            AppText(
              'You can keep signing in with your Mac password.',
              textAlign: TextAlign.center,
              style: TextStyle(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
                fontWeight: FontWeight.w600,
              ),
            ),
          ],
          if (gate.canPair) ...<Widget>[
            const SizedBox(height: 8),
            Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: <Widget>[
                Icon(Icons.shield_outlined, size: 16, color: Color(0xff8fa4bb)),
                SizedBox(width: 7),
                Flexible(
                  child: AppText(
                    'Protected by on-device security',
                    textAlign: TextAlign.center,
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.onSurfaceVariant,
                    ),
                  ),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }
}

class _SetupProgress extends StatelessWidget {
  const _SetupProgress({required this.state, required this.calibration});

  final DeviceState state;
  final CalibrationState calibration;

  @override
  Widget build(BuildContext context) {
    final gate = state.gate;
    return Container(
      key: const Key('setupProgress'),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 16),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surface,
        borderRadius: BorderRadius.circular(20),
        border: Border.all(color: Theme.of(context).colorScheme.outlineVariant),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Expanded(
            child: _ProgressStep(
              icon: Icons.shield_outlined,
              label: 'Secure',
              isComplete: gate.canPair,
            ),
          ),
          const _ProgressConnector(),
          Expanded(
            child: _ProgressStep(
              icon: Icons.link_rounded,
              label: 'Paired',
              isComplete: state.devices.isNotEmpty,
            ),
          ),
          const _ProgressConnector(),
          Expanded(
            child: _ProgressStep(
              icon: Icons.radar_rounded,
              label: 'Distance',
              isComplete: calibration.phase == CalibrationPhase.complete,
            ),
          ),
        ],
      ),
    );
  }
}

class _ProgressStep extends StatelessWidget {
  const _ProgressStep({
    required this.icon,
    required this.label,
    required this.isComplete,
  });

  final IconData icon;
  final String label;
  final bool isComplete;

  @override
  Widget build(BuildContext context) {
    final color = isComplete
        ? Theme.of(context).colorScheme.primary
        : Theme.of(context).colorScheme.onSurfaceVariant;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        Icon(isComplete ? Icons.check_circle_rounded : icon, color: color),
        const SizedBox(height: 7),
        AppText(
          label,
          textAlign: TextAlign.center,
          style: Theme.of(context).textTheme.labelMedium?.copyWith(
            color: color,
            fontWeight: FontWeight.w600,
          ),
        ),
      ],
    );
  }
}

class _ProgressConnector extends StatelessWidget {
  const _ProgressConnector();

  @override
  Widget build(BuildContext context) {
    return const Expanded(
      child: Padding(
        padding: EdgeInsets.only(top: 11),
        child: Divider(height: 1),
      ),
    );
  }
}

class _PairingCard extends StatelessWidget {
  const _PairingCard({
    required this.gate,
    required this.controller,
    required this.isOpeningScanner,
    required this.onScan,
  });

  final CapabilityGate gate;
  final PairingController controller;
  final bool isOpeningScanner;
  final VoidCallback onScan;

  bool get _busy => controller.state.isBusy;

  @override
  Widget build(BuildContext context) {
    final state = controller.state;
    final canScan =
        gate.canPair &&
        !_busy &&
        !isOpeningScanner &&
        state.phase != PairingPhase.awaitingConfirmation;
    return _SectionCard(
      icon: Icons.qr_code_scanner_rounded,
      title: 'Pair this phone',
      children: <Widget>[
        const AppText(
          'Open Repose on your Mac, create a one-time pairing QR code, then scan it here.',
        ),
        const SizedBox(height: 10),
        FilledButton.icon(
          key: const Key('scanPairingQrButton'),
          onPressed: canScan ? onScan : null,
          icon: isOpeningScanner
              ? const SizedBox.square(
                  dimension: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Icon(Icons.qr_code_scanner_rounded),
          label: const AppText('Scan Mac QR code'),
        ),
        if (state.phase == PairingPhase.awaitingConfirmation) ...<Widget>[
          const SizedBox(height: 10),
          _LiveStatus(
            'Confirm ${state.deviceName} before pairing',
            style: Theme.of(context).textTheme.titleSmall,
          ),
          const SizedBox(height: 6),
          FilledButton(
            key: const Key('confirmPairingButton'),
            onPressed: gate.canPair && !_busy
                ? () => unawaited(controller.confirmPairing())
                : null,
            child: const AppText('Confirm this device'),
          ),
        ],
        if (state.phase == PairingPhase.paired) ...<Widget>[
          const SizedBox(height: 8),
          const _LiveStatus('Pairing confirmed'),
        ] else if (state.message != null) ...<Widget>[
          const SizedBox(height: 8),
          _LiveStatus(state.message!),
        ],
      ],
    );
  }
}

class _CalibrationCard extends StatelessWidget {
  const _CalibrationCard({
    required this.gate,
    required this.hasPairedDevice,
    required this.controller,
  });

  final CapabilityGate gate;
  final bool hasPairedDevice;
  final CalibrationController controller;

  @override
  Widget build(BuildContext context) {
    final state = controller.state;
    final canMutate = gate.canCalibrate && hasPairedDevice && !state.isBusy;
    return _SectionCard(
      icon: Icons.social_distance,
      title: 'Distance calibration',
      children: <Widget>[
        const AppText(
          'Collect near for about 8 seconds, then far for 8 seconds.',
        ),
        if (!hasPairedDevice) ...<Widget>[
          const SizedBox(height: 6),
          const AppText('Pair a phone before calibration.'),
        ],
        const SizedBox(height: 8),
        FilledButton.icon(
          key: const Key('startCalibrationButton'),
          onPressed: canMutate
              ? () => unawaited(controller.startCalibration())
              : null,
          icon: const Icon(Icons.tune),
          label: AppText(
            state.canRetry ? 'Retry calibration' : 'Start calibration',
          ),
        ),
        if (state.phase == CalibrationPhase.collectingNear) ...<Widget>[
          const SizedBox(height: 8),
          FilledButton(
            key: const Key('submitNearButton'),
            onPressed: canMutate
                ? () => unawaited(controller.submitStep(CalibrationStep.near))
                : null,
            child: const AppText('Finish near sample'),
          ),
        ],
        if (state.phase == CalibrationPhase.collectingFar) ...<Widget>[
          const SizedBox(height: 8),
          FilledButton(
            key: const Key('submitFarButton'),
            onPressed: canMutate
                ? () => unawaited(controller.submitStep(CalibrationStep.far))
                : null,
            child: const AppText('Finish far sample'),
          ),
        ],
        if (state.message != null) ...<Widget>[
          const SizedBox(height: 8),
          _LiveStatus(state.message!),
        ],
      ],
    );
  }
}

class _DevicesCard extends StatelessWidget {
  const _DevicesCard({required this.controller, this.onAddDevice});

  final DeviceController controller;
  final VoidCallback? onAddDevice;

  @override
  Widget build(BuildContext context) {
    final state = controller.state;
    return _SectionCard(
      icon: Icons.devices,
      title: 'Paired devices',
      children: <Widget>[
        if (state.devices.isEmpty)
          const AppText('No paired devices yet.')
        else
          for (final device in state.devices)
            ListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(device.displayName),
              subtitle: AppText(
                device.platform == CompanionPlatform.android
                    ? 'Android'
                    : 'iOS',
              ),
              trailing: IconButton(
                tooltip: tr(context, 'Revoke ${device.displayName}'),
                onPressed: state.isMutationBusy
                    ? null
                    : () => _confirmRevocation(context, device),
                icon: const Icon(Icons.link_off),
              ),
            ),
        if (state.devices.isNotEmpty && onAddDevice != null) ...<Widget>[
          const SizedBox(height: 4),
          OutlinedButton.icon(
            key: const Key('addAnotherKeyButton'),
            onPressed: onAddDevice,
            icon: const Icon(Icons.add_rounded),
            label: const AppText('Add another key'),
          ),
        ],
        if (state.message != null) ...<Widget>[
          const SizedBox(height: 6),
          _LiveStatus(state.message!),
        ],
      ],
    );
  }

  Future<void> _confirmRevocation(
    BuildContext context,
    PairedDevice device,
  ) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const AppText('Revoke phone key?'),
        content: AppText('${device.displayName} will need to pair again.'),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const AppText('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const AppText('Revoke'),
          ),
        ],
      ),
    );
    if (confirmed == true) {
      await controller.revokeDevice(device.id);
    }
  }
}

class _SectionCard extends StatelessWidget {
  const _SectionCard({
    required this.icon,
    required this.title,
    required this.children,
  });

  final IconData icon;
  final String title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: EdgeInsets.zero,
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            Row(
              children: <Widget>[
                Container(
                  padding: const EdgeInsets.all(10),
                  decoration: BoxDecoration(
                    color: Theme.of(context).colorScheme.surfaceContainerLow,
                    borderRadius: BorderRadius.circular(12),
                  ),
                  child: Icon(
                    icon,
                    size: 21,
                    color: Theme.of(context).colorScheme.primary,
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: AppText(
                    title,
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 10),
            ...children,
          ],
        ),
      ),
    );
  }
}

class _LiveStatus extends StatelessWidget {
  const _LiveStatus(this.message, {this.style});

  final String message;
  final TextStyle? style;

  @override
  Widget build(BuildContext context) {
    return Semantics(liveRegion: true, child: AppText(message, style: style));
  }
}
