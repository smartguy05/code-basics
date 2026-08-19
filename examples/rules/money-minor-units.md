---
id: money-minor-units
title: Money is stored in minor units
---
Every monetary amount is stored and passed around as an integer count of the
currency's **minor unit** (e.g. cents for USD), never as a floating-point
value. Floats lose pennies to rounding and must not represent money.

- Persisted columns and DTO fields for money are integers.
- Arithmetic on money stays in minor units; divide only at the very end.
- Conversion to a decimal or a display string happens **only** at the
  presentation edge, never in domain or persistence code.

A diff that introduces a `float`/`double`/`decimal`-typed money field, or that
multiplies or divides a money value mid-calculation, violates this rule.
