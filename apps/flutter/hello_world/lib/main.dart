import 'package:flutter/material.dart';

void main() {
  runApp(const HyzHelloApp());
}

class HyzHelloApp extends StatelessWidget {
  const HyzHelloApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'hyz_things Flutter Hello',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xff4f46e5),
          brightness: Brightness.dark,
        ),
        useMaterial3: true,
      ),
      home: const Scaffold(
        body: Center(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.memory, size: 88),
              SizedBox(height: 24),
              Text(
                'Hello from Flutter!',
                style: TextStyle(fontSize: 40, fontWeight: FontWeight.bold),
              ),
              SizedBox(height: 12),
              Text(
                'hyz_things · RK3568 · Wayland',
                style: TextStyle(fontSize: 22),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
