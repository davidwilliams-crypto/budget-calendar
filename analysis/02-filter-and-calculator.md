# 02 — Transaction filter language and formula calculator

Sources: `ca.mishell.app.budget.Filter`, `ca.mishell.misc.Calculator`, `ca.mishell.misc.SimpleDoubleParser`, `ca.mishell.misc.Expression`, plus the call sites in `Budget.part01..part06` and `Payment`/`PaymentFlag`/`SimpleCalendar`.

## 0. Which engines the app actually uses

| Class | Referenced from Budget code? | Verdict |
|---|---|---|
| `Filter` | yes — `new Filter(...)` in part01 (lines 790, 1358, 2166, 2186, 2966); `isMatch` in part03/04/05 | live |
| `Calculator` | yes — `Calculator.calculate(...)` in part02 (101, 2188) and part04 (1949, 2157) | live |
| `SimpleDoubleParser` | no reference anywhere outside its own file | dead code |
| `Expression` | no reference anywhere outside its own file | dead code |
| `StockPrice` | no reference outside its own file (also not from `Expression`) | dead / unreachable |
| `JSONObject` | no reference outside its own file (also not from `StockPrice`/`Expression`) | dead / unreachable |

The grep was `Expression\.|StockPrice\.|Calculator\.|SimpleDoubleParser\.|JSONObject\.` and then a case-sensitive class-name grep over every pseudo file; the only hits for the four dead classes are inside themselves. The in-app help only talks about `Filter` and about "formulas" (= `Calculator`).

---

## 1. Filter language (`ca.mishell.app.budget.Filter`)

### 1.1 Where it is used

- Calendar view filter box (`tranFilterTxt`): on Enter (`Budget$21.widgetDefaultSelected`) the text is `trim()`ed, every `&` is replaced by `＆` (U+FF06, `FAKE_AMPERSAND` — the app stores `&` in descriptions as U+FF06 because SWT uses `&` as a mnemonic), and if non-empty `calViewFilter = new Filter(text)`; empty text → `calViewFilter = null` (no filtering).
- "All payments" dialog has its own filter text, stored on the table as `"filter"` data (same `&` substitution); `fillPaymentsTable` calls `isMatch(pmt, pmt.amount, 0)`.
- Click helpers generate filters: clicking an amount label → `new Filter("desc:" + description.replaceAll(" ", "\\ "))`; clicking a stats row → `">0"` (Total Income), `"<0"` (Total Expenses), or `"cat:" + category with spaces escaped as "\ "`; a menu item sets the text box to `desc:<desc with "\ ">`.
- Statistics (`part05` ~2362/2390): regular payments are tested with `isMatch(pmt, pmt.amount, -1)`; daily transactions with `isMatch(pmt, tran.amount, dayYYYYMMDD)`.
- The constructor is **not** wrapped in try/catch at any call site.

Tooltip (`Budget.filterToolTipText`, part06:2270) — authoritative for v1.11:

```
Filter expression format:
[-][|] <[~]token>[ <[~]token> [...]]
  '-' filters and displays DELETED transactions only -- can be used without pararameters
  '|' sets the 'OR' search mode (the default is the 'AND' mode );
  '~' inverts its token condition
token:
-- >n, <n or =n -- n is the amount
-- 'cat:<substring>' to search categories only
-- 'note:<substring>' to search notes only
-- 'desc:<substring>' to search description only
-- 'date:<date>' to search date only
-- 'range:<n|active|past|future|disabled|i[ndefinite]>' to filter-out by date range
-- '[flag]:<paid or .|unpaid or ?|important or !> -- filter paid, unpaid, important.
Misc:
-- Use '\ ' when searching for a space.
```

The HTML manual (`budget.html`, "Filter expression" section) is older: it says "Use '_' where you need a filter for a space". **The code contains no `_` handling at all** — `_` is an ordinary literal character. Only `\ ` (backslash-space) escapes a space.

### 1.2 Parsing — `Filter.<init>(String)`

Fields: `OR_mode` (bool), `deleted` (bool), `filter` (original text), `elms` (Vector of `elm`).

1. `deleted = text.startsWith("-")`; if so strip the first char.
2. If the (remaining) text is non-empty and its first char is `|` → `OR_mode = true`, strip it. Order matters: `-|a b` = deleted + OR; `|-a` = OR with a literal token `-a`.
3. `text = text.toLowerCase()` (default locale), then `replaceAll("\\\\ ", NBSPACE)` — i.e. every backslash-space pair becomes U+2029. The whole expression is lowercased, so matching is **case-insensitive** everywhere.
4. Split on `' '` with `BudgetDB.split` (plain split on every space; consecutive spaces produce empty strings; a trailing space leaves a `null` last slot).
5. If the array is null/empty, or **the first token is empty** (e.g. `"-  cat:x"` → after stripping `-` the text starts with a space) → return with **no elements** (matches everything; only the `deleted`/`OR_mode` flags survive). Later empty/null tokens are simply skipped.
6. For each non-empty token:
   - replace U+2029 back to a real space;
   - `invert = token.startsWith("~")`; if so strip one `~` (a token of just `~` then does `"".charAt(0)` → `StringIndexOutOfBoundsException`, uncaught (verified against regenerated pseudo code) — UNKNOWN what SWT does with it, see list);
   - first char `<` → `LT` elm with `exp = rest`; `=` → `EQ`; `>` → `GT`;
   - else if the token contains `:` → `name = names.indexOf(prefix before first ':')` (`-1` = UNKNOWN if prefix not in `all,cat,desc,date,lt,eq,gt,note,range,flag`), `exp = text after the first ':'`;
   - else → `ALL` elm with `exp = token`.

Name codes: `UNKNOWN=-1, ALL=0, CAT=1, DESC=2, DATE=3, LT=4, EQ=5, GT=6, NOTE=7, RANGE=8, FLAG=9`. Note that `lt:100`, `eq:100`, `gt:100` are also accepted via the prefix form, and that `flag:` is optional: a bare `:paid` yields name `-1`, which the matcher treats identically to `FLAG`.

`elm` constructor: `isDots = exp.contains(".")`; `val` is computed **only** for names `ALL, LT, EQ, GT, RANGE` as `Budget.toDouble(exp)`; other names keep `val = 0.0`.

`Budget.toDouble(s)`: strip a leading `+`, remove all `$`, then `numFormatFrac2.parse(s).doubleValue()`; any exception → `0.0`. `numFormatFrac2` is `DecimalFormat.getNumberInstance()` for the default locale with grouping on, min/max fraction digits 2. `NumberFormat.parse(String)` is **prefix-lenient**: it parses the longest leading number and ignores the rest, so `toDouble("2026-10") = 2026`, `toDouble("1,500abc") = 1500`, `toDouble("abc") = 0`, `toDouble("") = 0`, `toDouble("-100") = -100`. Decimal/grouping separators are those of the JVM default locale.

### 1.3 Matching — `isMatch(Payment pmt, double amount, int date)`

`amount` is the (possibly adjusted) occurrence amount; `date` is the occurrence day as `yyyymmdd` int, or `0` (All-payments dialog) or `-1` (regular-payment stats pass). The function does **not** apply `deleted` — that flag is read by the callers to decide which population to show.

- No elements → `true`.
- Otherwise iterate elements in order; per element compute `v11` (matched) as below, then apply `invert`, then: in AND mode a `false` returns `false` immediately; a `true` increments a counter. At the end return `counter > 0`. So AND = every element true; OR = at least one true.

Per-element evaluation. `v9` = haystack string (or null), `v10` = "numeric/boolean element, skip substring logic", `v11` = result.

| name | what is computed |
|---|---|
| `ALL` (0) | `v9` = `sdate(pmt.startDate) + " " + description + " " + s2s(category) + " " + numFormatFrac2.format(amount) + " " + s2s(getNote(date))`, lowercased. Built once per `isMatch` call and reused by every `ALL` element. |
| `CAT` (1) | `v9 = category.toLowerCase()`; if category is null → `v9 = null` (element false unless inverted). |
| `DESC` (2) | `v9 = description.toLowerCase()`. |
| `DATE` (3) | `v9 = sdate(pmt.startDate).toLowerCase()`. Matches the **payment's start date**, not the occurrence date. |
| `LT` (4) | `v10 = 1; v11 = amount < val` |
| `EQ` (5) | `v10 = 1; v11 = val == amount` (exact double equality) |
| `GT` (6) | `v10 = 1; v11 = amount > val` |
| `NOTE` (7) | `v9 = getNote(date)` lowercased; null → `v9 = null`. |
| `RANGE` (8) | `v10 = 1`; see 1.5 |
| `FLAG` (9) and `UNKNOWN` (-1) | see 1.6; `v10 = 1` only when `exp` is a recognised flag word |
| anything else | `return false` (unreachable in practice) |

Common tail (only when `v10 == 0` and `v9 != null`):

1. `v11 = v9.contains(exp)` (plain substring, already lowercased on both sides).
2. If not matched and `val != 0.0` → `v11 = (amount == val)` (exact equality). Only `ALL` elements can reach this with a non-zero `val`.
3. Else if not matched, `!isDots` and `name == ALL`: build `t = (description + " " + s2s(category) + " " + s2s(getNote(date))).toLowerCase()`; if `t` contains a `.` → `v11 = t.replaceAll("\\.", "").contains(exp)`. I.e. a dot-less search term also matches text with the dots removed (`stjohn` matches `St.John`), but only in `ALL` mode, and the date/amount are not part of that second haystack.

`s2s(String)`: null → `""`, else the string unchanged.

Strings used by the haystacks:

- `Budget.sdate(int yyyymmdd)` = `new SimpleDateFormat("dd-MMM-yyyy").format(date)`, default locale. In an English locale: `05-Oct-2026` → lowercased `05-oct-2026`. Therefore `date:2026-10` **never matches**; `date:oct-2026`, `date:-2026`, `date:05-oct` do. (Locale caveat: CLDR data for some English variants, e.g. `en_CA`, abbreviates months with a trailing period such as `Oct.`; UNKNOWN which locale the shipped runtime resolves — see list.)
- `numFormatFrac2.format(amount)`: grouping on, exactly two decimals, locale symbols; e.g. `-1,234.50`, `100.00`.
- `Payment.getNote(date)`: if the payment has notes: `date == 0` → all notes concatenated, each followed by a space; otherwise the note whose `date` equals the argument, or `null`. No notes → `null`.
- `Payment.getFlag(date)`: the `PaymentFlag.value` whose `date` equals the argument, else `pmt.flag_value` (the payment-wide default). Values: `'^'` (94) `FLAG_NONE`, `';'` (59) `FLAG_PAID`, `'#'` (35) `FLAG_IMPORTANT`.

### 1.4 Amount operators

`<n`, `=n`, `>n` and `lt:n`, `eq:n`, `gt:n`. `n` goes through `toDouble`, so `+`, `$` and locale grouping are accepted and garbage parses as `0`: `<` alone means `amount < 0`, `=` alone means `amount == 0`, `>abc` means `amount > 0`. Comparisons are against the occurrence amount passed to `isMatch` (expenses are negative).

### 1.5 `range:` values

`today = new SimpleCalendar().get()` (current date as `yyyymmdd`), `indefinite = (pmt.flags & 0xF0) == END_MODE_INDEFINITE (16)`.

Evaluation order (first match wins):

1. `exp == "disabled"` or `"d"` → `v11 = pmt.disabled`.
2. `val != 0.0` (exp parsed as a non-zero number, e.g. `range:2026`; checked **before** the keywords, so `range:1active` is numeric):
   `v11 = year(startDate) < val && (indefinite || val <= year(endDate))` — note the **strict** `<` on the start year (so a payment that starts in 2026 does not satisfy this part of `range:2026`; replicate as-is).
   If that is false: `v11 = pmt.getOpt(val*10000 + 65, val*10000 + 1231) != null`, i.e. the payment has at least one `PaymentOption` (per-date adjustment/exception record) whose `date` lies in `[yyyy0065, yyyy1231]`; since `yyyy0065 < yyyy0101` numerically this is simply "any option dated in year `val`".
   `year(d)` is `SimpleCalendar.getYear` = `normalizedDate(d) / 10000`.
3. `"active"` / `"a"` → `startDate <= today && (indefinite || today <= endDate)`.
4. `"past"` / `"p"` → `endDate < today && !indefinite`.
5. `"future"` / `"f"` → `today < startDate`.
6. `"indefinite"` / `"i"` → `indefinite`.
7. anything else (including `range:0`, `range:`) → `false`.

`range:` is always boolean (`v10 = 1`): no substring fallback.

### 1.6 `flag:` values (also bare `:value` or any unknown `xxx:value` prefix)

`f = pmt.getFlag(date)`.

- `?` or `unpaid` → `v10 = 1; v11 = (f == '^' || f == '#')` — "unpaid" includes important-but-unpaid.
- `.` or `paid` → `v10 = 1; v11 = (f == ';')`.
- `!` or `important` → `v10 = 1; v11 = (f == '#')`.
- anything else → `v10 = 0`, `v9 = null` → `v11 = false` (so `foo:bar` with an unknown prefix is always false; `~foo:bar` always true).

Because the whole expression was lowercased, the `equalsIgnoreCase("UNPAID")` etc. are effectively plain comparisons. With `date == 0` or `-1` there is never a matching `PaymentFlag`, so the payment default `flag_value` is tested.

### 1.7 Worked examples

Assume payment P: description `Hydro bill`, category `Utils`, startDate 2025-03-01, endDate 2027-12-31 (END_MODE_DATE), amount −120.00, no notes, flag `^`; today = 2026-10-27; the calendar passes `amount = -120`, `date = 20261105`.

- `cat:utils <-100` → AND mode, two elements: `CAT "utils"` → `"utils".contains("utils")` true; `LT -100` → `-120 < -100` true → **match**. (`Hydro bill` at −80 would fail the second.)
- `|salary rent` → OR mode, `ALL "salary"`, `ALL "rent"`. Haystack for P is `"01-mar-2025 hydro bill utils -120.00 "`; neither substring found, `val` for both is 0, no dots in `"hydro bill utils "` → both false → **no match**. A payment described `Rent` matches via the second element.
- `-` → `deleted = true`, text becomes `""` → split gives `[""]`, first token empty → no elements → `isMatch` returns true for everything; the callers use `deleted` to list only deleted transactions.
- `range:future ~cat:auto` → AND: `range:future` → `20261027 < 20250301` false → return **false** immediately (the second element is never evaluated). For a payment starting 2027-01-01 in category `Auto`: first true, second `"auto".contains("auto")` = true, inverted → false → no match; in category `Fuel` → match.
- `flag:paid` → `getFlag(20261105)`; if that day was marked paid (`;`) → match, otherwise (default `^`) → no match. `:paid` and `anything:paid` behave the same. `~flag:paid` lists everything not paid.
- `date:2026-10` → haystack `"01-mar-2025"` (start date, `dd-mmm-yyyy`); `"2026-10"` is never a substring of that format → **never matches** anything. `date:mar-2025` matches P; `date:2025` matches P.
- `120` (ALL) → haystack contains `"-120.00"` which contains `"120"` → match. `=120` → `-120 == 120` false → no match; `=-120` → match.
- `hydro\ bill` → token becomes `hydro bill` → `ALL` substring → match. `hydro_bill` → literal underscore → no match.
- `stjohn` vs description `St.John` → direct search fails, `val = 0`, `!isDots`, text has a `.` → dots removed → `"stjohn"` found → match. `st.john` (`isDots`) only matches the literal.
- `range:2026` for P → `2025 < 2026 && 2026 <= 2027` → true. For a payment starting 2026-05-01 with no options → `2026 < 2026` false, no options → false (quirk of the strict comparison).

### 1.8 Re-implementation notes

- Lowercase and split before interpreting; preserve the "first token empty → no elements" quirk and the `-`/`|` ordering.
- `toDouble` must be prefix-lenient and locale-aware (or pick a fixed `en` behaviour and document the divergence).
- The `ALL` haystack must be built with exactly the pieces and separators above (`sdate`, description, category-or-empty, 2-decimal grouped amount, note-or-empty) because amount searches rely on the formatted string.
- `date:` compares against the start date formatted `dd-MMM-yyyy`, lowercased.

---

## 2. Formula calculator (`ca.mishell.misc.Calculator`)

### 2.1 Entry points and callers

`calculate(String expr)` = `calculate(expr, 0.0, 0.0)`; `calculate(String expr, double weekIncome, double monthIncome)` (both `synchronized` static; return `double`; throw checked `Exception` with the messages below).

| Call site | Args | Behaviour around the call |
|---|---|---|
| `Budget$?.focusLost` on the amount `Text` in the payment editor (part02:101) | `(text, 0, 0)` | text trimmed; a leading `+` selects the Income radio and is stripped; result → `abs` formatted with `numFormatFrac2` back into the field; negative result selects Expense; on exception the field turns red (system colour 3) and keeps focus. |
| Balance editor OK (part02:2188) | `(balanceText, 0, 0)` | exception → `showError("Invalid amount!\n" + message)`, refocus/select-all. |
| Balance adjustment prompt (part04:1949) "Enter new balance adjustment (formulas are allowed)" | `(input, 0, 0)` | exception → `showError(message)`. |
| Amount adjustment prompt (part04:2157) "Enter the new amount, formulas are allowed… Use: %n to calculate week's income percentage, or %%n month's income percentage." | `(input, weekIncome, monthIncome)` | `weekIncome`/`monthIncome` are computed **only if the input contains `%`**, otherwise both are `0`. Input `$` or `$$` is intercepted before the calculator (best-saving helper). If the result `> 0` and the Expense button was used, the result is negated. If the input contained `%`, the computed value is formatted and the prompt is shown again with it (so the user sees the number before accepting). |

`getWeekIncome(cal)`: walk back to the configured first day of week, then sum `tran.amount` over 7 days for transactions whose payment is not disabled and whose amount `> 0`. `getMonthIncome(cal)`: same over every day of the calendar month. Both `private` in `Budget` (part04:2199/2229).

### 2.2 Tokenizer

`new StringTokenizer(expr + ETX, " \t+-/*()" + ETX, returnDelims = true)` where `ETX = "⎆"`. Delimiters are returned as single-character tokens; everything else is a run of non-delimiter chars. `next_token` trims each token, deletes every `$`, and skips tokens that are then empty (so blanks vanish). Consequences:

- `,` and `.` and `%` are **not** delimiters: `1,000.50`, `%5`, `%%12.5`, `1.5k`, `$1,000` are single tokens.
- `-` and `+` are always separate tokens (so `-5` is the token `-` followed by `5`; `1e-3` is `1e`, `-`, `3`).
- `*` is a delimiter, so `**` is two tokens.

### 2.3 Numbers — `isNumber(String s, CalcDouble out)`

1. Remove every `%` from `s`.
2. If `s` starts with `+`: a lone `+` is not a number; otherwise parsing starts at index 1. Else start at 0.
3. `parseFmt.parse(s, pos)` with `parseFmt = DecimalFormat.getNumberInstance()` (default locale; grouping accepted; prefix-lenient). `null` result → not a number.
4. If the parse stopped before the end: if exactly one character remains it must be a multiplier suffix `k`/`K` ×1 000, `m`/`M` ×1 000 000, `b`/`B` ×1e9, `t`/`T` ×1e12; any other trailing text → not a number.
5. `out.doubleValue = parsed * multiplier`; return true.

Examples: `5` → 5; `1,500` → 1500 (en locale); `2.5k` → 2500; `1m` → 1e6; `5%` → 5 (the `%` is just dropped here); `5x` → not a number; `+` → not a number; `+5` → 5.

### 2.4 Grammar and evaluation — `process_expr(valueStack, tokens, week, month)`

Shunting-yard with a value stack (`Stack<Double>`) and an operator stack per parenthesis level. State `expectOperand` starts `true`.

**Expecting an operand** (`expectOperand == true`):

1. If the token is `-` or `+` (unary sign; `sign = 0` for `-`, `1` for `+`, `-1` for none): read the next token; `null` → "Unexpected end of expression"; it must satisfy `isNumber` **or** be `(` — otherwise "Syntax error near 'tok'". (`isNumber` strips `%`, so `-%10` and `-%%10` are legal; `--5` is not.)
2. Token `(` → recurse `process_expr` (consumes up to and including the matching `)` / ETX). If the sign was `-`, push `-1` and apply `*` (negation). `expectOperand = false`.
3. Token starting with `%%` → `value = parseFmt.parse(tok.substring(2)) * monthIncome / 100 * (sign == '-' ? -1 : 1)`; token starting with `%` → same with `tok.substring(1)` and `weekIncome`. The double is converted to a string and fed to `calc`, which re-parses it with `isNumber` (see caveat below). `expectOperand = false`. A bare `%` or `%abc` makes `parseFmt.parse` throw `ParseException` ("Unparseable number: ...").
4. Otherwise the token must satisfy `isNumber` (else "Syntax error near 'tok'"); `calc(stack, ("-" if sign=='-' else "+") + tok)` pushes the signed value. `expectOperand = false`.

**Expecting an operator** (`expectOperand == false`):

1. Token `)` or ETX → pop and apply every operator on this level's operator stack, then return. (A stray `)` at top level also returns; `calculate` then sees leftover tokens and reports "Unexpected end of expression". A missing `)` is consumed by the inner level, after which the outer level hits end-of-tokens → same message.)
2. `weight(tok)` must not be `-1` (else "Syntax error near 'tok'"): `+`,`-` → 1; `*`,`/` → 2; anything else → −1. Only the first character is examined.
3. While the operator stack is non-empty and `weight(top) >= weight(tok)`: pop and apply `top`. Then push `tok`. `expectOperand = true`.

So precedence is the usual two levels, left-associative; no exponent, no modulo, no implicit multiplication, no function calls.

**`calc(stack, s)`**: if `isNumber(s)` → push. Else requires two values ("Not enough operands in the stack"), pops `b` then `a`, and by first char: `+` → `a+b`, `-` → `a-b`, `*` → `a*b`, `/` → `b == 0` ? throw "Division by zero!" : `a/b`; `,`/`.`/other → pushes `a` (unreachable because `weight` filters operators).

**`calculate`** after `process_expr`: value stack must hold exactly one value and the tokenizer must be exhausted, else "Unexpected end of expression". Empty/blank input → "Empty expression".

Error messages (exact): `Empty expression`, `Unexpected end of expression`, `Syntax error near '<tok>'`, `Not enough operands in the stack`, `Division by zero!`, plus Java's `ParseException` text for a bad `%n` number.

### 2.5 Worked examples (`en` locale, week = 1000, month = 4300)

| Input | Result | Notes |
|---|---|---|
| `100+20*3` | 160 | `*` binds tighter |
| `(100+20)*3` | 360 | |
| `-(5+5)*2` | −20 | unary minus on a group = `*-1` |
| `$1,200.50 - 200` | 1000.5 | `$` stripped; grouping parsed |
| `2.5k/2` | 1250 | suffix multiplier |
| `%10` | 100 | 10 % of week income |
| `%%25 + 100` | 1175 | 25 % of month income |
| `-%10` | −100 | sign before `%` allowed |
| `10%` | 10 | `%` only has meaning as a prefix; elsewhere it is deleted by `isNumber` |
| `5 5` | error "Syntax error near '5'" | operand where operator expected |
| `5+` | error "Unexpected end of expression" | `+` is read as an operator, then ETX arrives where an operand is expected; `syntax_exception(ETX)` maps to this message |
| `10/0` | error "Division by zero!" | |
| `` (blank) | error "Empty expression" | |

### 2.6 Caveats to replicate or consciously drop

- `%n` results are round-tripped through `String.valueOf(double)` and `DecimalFormat.parse`; values that print in exponent form (`1.0E-4`) or in a locale whose decimal separator is not `.` would fail or truncate. A clean re-implementation should push the double directly.
- Parsing is locale-dependent (`DecimalFormat.getNumberInstance()`): grouping/decimal symbols follow the JVM default locale.
- `isNumber` is prefix-lenient except that it rejects any trailing text other than a single multiplier letter; `1e3` is accepted only if the locale's `DecimalFormat` parses exponents (Java's does: `1E3` → 1000).

---

## 3. `ca.mishell.misc.SimpleDoubleParser` (dead code)

`static double parseDouble(String s)` — hand-written, locale-free parser, **not referenced by the app**:

- length `> 17` → `NumberFormatException("invalid number: string is too long '…'")`; empty → `"invalid number: string is empty"`.
- Optional single leading `-` (negates) or `+`; nothing after the sign → `"invalid number: '…'"`.
- Integer digits accumulate as `acc = acc*10 + d`. A `.` ends the integer part; `.` as the last char → `"invalid number: no digits after decimal separator in '…'"`; any other non-digit → `"invalid number: unexpected character '<c>'"`.
- Fraction digits: `acc = acc*10 + d`, `div *= 10`; non-digit → `"invalid number: unexpected character: '<c>'"`.
- Result `±acc` (no dot) or `±acc / div`. No grouping, no exponent, only `.` as decimal point.

---

## 4. `ca.mishell.misc.Expression` (dead code, summary only)

A general-purpose, 88 KB expression engine (`Expression.calculate(String, VariableProvider, FunctionProvider[, MathContext])` returning a `VarValue`) with its own tokenizer, `Type` enum of ~40 token kinds (POWER, REGEX/NREGEX, MULTIPLY, DIV, MODULO, PLUS, MINUS, shifts, GT/GE/LE/LT/EQ/NE, LIKE/NOT_LIKE, BIT_AND/XOR/OR, LOGIC_AND/OR, ASSIGNMENT, FUNC, STRING, NUMBER, LIST, VARIABLE, …), `BigDecimal` arithmetic (`MathContext.DECIMAL128`), string/regex/list comparisons (case-insensitive), variables via a `VariableProvider`, functions via a `FunctionProvider`, date literals `` `YYYY-MMM-DD` `` → `YYYYMMDD` numbers, and a `Stats` object reporting which vars/functions were hit. Its `getSyntaxDescription()` documents: grouping with `!` negation, `||`/`&&`, `name = expr` assignment (and comma-separated multiple assignments, last one is the result), text in `"…"`/`'…'`, numbers per `DecimalFormat.parse` (so `func(10,10)` is one argument `1010`), `{…}` string lists, math ops `+ - * / % | & ^` and `**` integer power, comparisons `== != > < >= <=`, `=#`/`!=#` "contains in text or list", `=~`/`!=~` Java-regex match, precedence `* / %` > `+ -` > `> < >= <=` > `[!]=#` `[!]=~` > `& ^ |` > `= !=` > `&&` > `||`. **No Budget class references it**, so none of this needs to be re-implemented. `StockPrice` (Yahoo Finance v8 chart JSON fetcher with a cache) and `JSONObject` (reflection-based JSON builder/parser) are likewise unreferenced from Budget, `Filter`, `Calculator` or `Expression`.

---

## UNKNOWN

1. `Filter.<init>` — a token consisting only of `~` (or `~` after `\ ` tricks) throws `StringIndexOutOfBoundsException` (the throw is confirmed: `substring(1)` then `charAt(0)` on the empty string — verified against regenerated pseudo code); the call sites have no try/catch, so the observable UI behaviour (SWT swallowing vs. crash dialog) is UNKNOWN.
2. `Budget.sdate` / `Filter.isMatch(DATE, ALL)` — exact month abbreviations depend on the JVM default locale and its CLDR data (e.g. `Sep` vs `Sept`, `Oct` vs `Oct.`); which locale the shipped bundle runs with is UNKNOWN. English `dd-MMM-yyyy` (`05-Oct-2026`) is assumed.
3. `Filter.isMatch(RANGE, numeric)` — the lower bound `val*10000 + 65` is really the constant 65 (verified against regenerated pseudo code) (expected `+101`); behaviourally irrelevant, but the intent of the original constant is UNKNOWN.

(`BudgetDB.split`: the regenerated pseudo code renders the store as `$t = v2; v2 += 1; v5[$t] = …` — a conventional post-increment, index 0 first; no longer an open question.)

Verified pass: 2026-10-27 — 1 statements corrected, 2 confirmed.
