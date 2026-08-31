# Notes

- Events already visible in the active workspace must not latch and flash later.
- Failure outranks attention and success; cancellation produces no signal.
- Only stable, invokable app commands belong in the editable shortcut catalog.
- Built-in themes are immutable; customization starts by duplicating one.
- Windows path literals in frontend tests must escape backslashes; `"C:\\ws"`
  is a different string from `"C:\ws"` in JavaScript source.
- Cancellation can race process reaping after a failed `taskkill`; ownership
  tokens distinguish a vanished target from a real kill failure.
