import 'package:flutter_test/flutter_test.dart';
import 'package:hyz_flutter_hello/main.dart';

void main() {
  testWidgets('renders the RK3568 greeting', (WidgetTester tester) async {
    await tester.pumpWidget(const HyzHelloApp());

    expect(find.text('Hello from Flutter!'), findsOneWidget);
    expect(find.text('hyz_things · RK3568 · Wayland'), findsOneWidget);
  });
}
