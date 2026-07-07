import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import 'window_controls.dart';

/// Кастомный title bar: drag-зона + оконные контролы.
class CustomTitleBar extends StatelessWidget {
  const CustomTitleBar({super.key});

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: 40,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          Expanded(
            child: GestureDetector(
              onPanStart: (_) => windowManager.startDragging(),
              onDoubleTap: () async {
                if (await windowManager.isMaximized()) {
                  await windowManager.unmaximize();
                } else {
                  await windowManager.maximize();
                }
              },
              behavior: HitTestBehavior.translucent,
              child: Container(),
            ),
          ),
          const WindowControls(),
        ],
      ),
    );
  }
}
