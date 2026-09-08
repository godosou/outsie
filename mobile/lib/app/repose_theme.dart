import 'dart:math' as math;
import 'package:flutter/material.dart';

// Shared with src/styles.css: warm daylight and the quiet forest palette.
ThemeData reposeTheme(Brightness brightness) {
  final dark = brightness == Brightness.dark;
  final background = Color(dark ? 0xff202b24 : 0xfffbfcf9);
  final panel = Color(dark ? 0xff26322a : 0xffffffff);
  final text = Color(dark ? 0xffd5dece : 0xff303b32);
  final muted = Color(dark ? 0xffabb9a0 : 0xff677160);
  final green = Color(dark ? 0xffa9c391 : 0xff526a43);
  final line = Color(dark ? 0xff43513e : 0xffe9ece4);
  final sage = Color(dark ? 0xff303e2b : 0xffeff3e9);
  final scheme = ColorScheme.fromSeed(seedColor: green, brightness: brightness)
      .copyWith(
        primary: green,
        onPrimary: dark ? const Color(0xff202b24) : Colors.white,
        surface: panel,
        onSurface: text,
        onSurfaceVariant: muted,
        surfaceContainerLow: sage,
        outlineVariant: line,
      );
  final base = ThemeData(
    brightness: brightness,
    colorScheme: scheme,
    useMaterial3: true,
  );
  return base.copyWith(
    scaffoldBackgroundColor: background,
    textTheme: base.textTheme.apply(
      fontFamily: 'Manrope',
      bodyColor: text,
      displayColor: text,
    ),
    appBarTheme: AppBarTheme(
      backgroundColor: background,
      foregroundColor: text,
      elevation: 0,
      scrolledUnderElevation: 0,
      centerTitle: false,
      toolbarHeight: 72,
    ),
    cardTheme: CardThemeData(
      color: panel,
      elevation: 0,
      margin: EdgeInsets.zero,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(22),
        side: BorderSide(color: line),
      ),
    ),
    dividerTheme: DividerThemeData(color: line, space: 24),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: background,
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(14),
        borderSide: BorderSide(color: line),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(14),
        borderSide: BorderSide(color: line),
      ),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        minimumSize: const Size.fromHeight(52),
        textStyle: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        minimumSize: const Size(48, 48),
        side: BorderSide(color: line),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
      ),
    ),
    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(minimumSize: const Size(48, 48)),
    ),
    iconButtonTheme: IconButtonThemeData(
      style: IconButton.styleFrom(minimumSize: const Size(48, 48)),
    ),
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
    ),
  );
}

class ReposeBrandMark extends StatelessWidget {
  const ReposeBrandMark({super.key, this.size = 34});
  final double size;
  @override
  Widget build(BuildContext context) => ExcludeSemantics(
    child: CustomPaint(
      size: Size.square(size),
      painter: _Petals(
        Theme.of(context).colorScheme.primary,
        Theme.of(context).scaffoldBackgroundColor,
      ),
    ),
  );
}

class _Petals extends CustomPainter {
  const _Petals(this.color, this.background);
  final Color color;
  final Color background;
  @override
  void paint(Canvas canvas, Size size) {
    canvas.translate(size.width / 2, size.height / 2);
    canvas.scale(size.width / 64);
    final paint = Paint()..color = color;
    for (final angle in [-40, 40, 130, 220]) {
      canvas.save();
      canvas.rotate(angle * math.pi / 180);
      canvas.drawOval(const Rect.fromLTWH(-7, -25, 14, 30), paint);
      canvas.restore();
    }
    canvas.drawCircle(Offset.zero, 5, Paint()..color = background);
  }

  @override
  bool shouldRepaint(_Petals oldDelegate) =>
      oldDelegate.color != color || oldDelegate.background != background;
}
