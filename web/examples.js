// Bundled example programs for the editor tab's sidebar.
//
// A guided tour: each example showcases one distinctive tigr feature,
// ordered simple-to-advanced. Keyed by the `data-value` on each
// `.example-item` button in index.html. Every program runs in the
// browser build (no Net / Os / filesystem; concurrency uses green
// threads, which run client-side).

export const EXAMPLES = {
  expressions: `// Everything is an expression — blocks, if, and loops all yield a value.

// A block evaluates to its last expression.
size := { base := 10; base * base };
print('size  =', size);                        // => 100

// 'if' is an expression, so its result can be bound to a name.
label := if size > 50 { 'big' } else { 'small' };
print('label =', label);                       // => big

// A plain 'for' yields its final iteration; 'for[]' collects them all.
print('last  =', for (n, 1..=5) { n * n });     // => 25
print('all   =', for[] (n, 1..=5) { n * n });   // => [1, 4, 9, 16, 25]

// Because every construct composes, control flow nests inside a value.
grades := for[] (score, [91, 50, 72]) {
  if score >= 60 { 'pass' } else { 'fail' }
};
print('grades =', grades);                      // => [pass, fail, pass]
`,

  pipelines: `// Pipelines — the pipe '|>' threads a value left-to-right, and lazy
// 'Iter' runs each element through the whole chain without ever
// building the intermediate arrays.

Iter := import 'Iter';

// 'x |> f(args)' is simply 'f(x, args)' — read top to bottom.
double := fn(x) { x * 2 };
print('piped =', 5 |> double() |> double());     // => 20

// take(3) pulls only three values, so the million-element source is
// never fully built — work happens strictly on demand.
first := Iter.from(1..=1000000)
  |> Iter.map(fn(n) { n * n })
  |> Iter.filter(fn(n) { n % 2 == 0 })
  |> Iter.take(3)
  |> Iter.collect();
print('first =', first);                         // => [4, 16, 36]

// reduce folds an entire pipeline down to a single value.
sum := Iter.from(1..=100) |> Iter.reduce(fn(a, b) { a + b }, 0);
print('sum 1..100 =', sum);                       // => 5050
`,

  match: `// 'match' is an expression — it yields the body of the first arm whose
// pattern fits, so it slots in anywhere a value is expected.

// Literal and range patterns, plus a bare name that binds the value.
classify := fn(n) {
  match n {
    0       => 'zero',
    1..=9   => 'one digit',
    10..=99 => 'two digits',
    big     => 'large: {big}',
  }
};
print(for[] (n, [0, 7, 42, 5000]) { classify(n) });
// => [zero, one digit, two digits, large: 5000]

// Patterns can match structure — here, objects as tagged variants.
area := fn(shape) {
  match shape {
    \${kind: 'circle', r}  => 3.14159 * r ^^ 2,
    \${kind: 'rect', w, h} => w * h,
    _                     => raise 'unknown shape',
  }
};
print('circle =', area(\${kind: 'circle', r: 2}));    // => 12.56636
print('rect   =', area(\${kind: 'rect', w: 3, h: 4})); // => 12
`,

  destructuring: `// Destructuring — bind several names at once by shape, both in ':='
// and in function parameters.

// Array patterns, with '...rest' to gather the tail.
[first, second, ...rest] := [10, 20, 30, 40, 50];
print('first={first} second={second} rest={rest}');
// => first=10 second=20 rest=[30, 40, 50]

// Object patterns match by key; 'key: name' renames as it binds.
\${name, age: years} := \${name: 'tigr', age: 3, extra: true};
print('name={name} years={years}');              // => name=tigr years=3

// Patterns nest, following the shape of the data.
\${user: \${id, name: who}} := \${user: \${id: 7, name: 'ada'}};
print('id={id} who={who}');                       // => id=7 who=ada

// A function parameter can itself be a pattern.
distance_sq := fn([x1, y1], [x2, y2]) {
  dx := x2 - x1;
  dy := y2 - y1;
  dx * dx + dy * dy
};
print('distance^2 =', distance_sq([0, 0], [3, 4]));  // => 25
`,

  closures: `// Closures capture by reference — a returned function keeps a live
// link to the variables it closed over, not a snapshot.

make_counter := fn() {
  n := 0;
  fn() { n = n + 1; n }        // this inner function captures 'n'
};

c := make_counter();
print(c(), c(), c());          // => 1 2 3

// Each call to make_counter creates a fresh 'n', so separate counters
// never interfere with each other.
a := make_counter();
b := make_counter();
a(); a();
print('a =', a(), ' b =', b());  // => a = 3  b = 1
`,

  generators: `// A 'gen fn' builds a paused coroutine. Calling it returns an iterator;
// each next() runs the body forward to the next 'yield'.

ramp := gen fn(n) {
  i := 0;
  while i < n {
    yield i;
    i = i + 1;
  };
};

// Pull values one at a time...
g := ramp(3);
print(g.next());                        // => \${done: false, value: 0}
print(g.next());                        // => \${done: false, value: 1}

// ...or let 'for' and spread drive the generator for you.
for (x, ramp(3)) { print('step', x) };   // => step 0 / step 1 / step 2
print('spread =', [...ramp(5)]);          // => [0, 1, 2, 3, 4]

// Generators can be infinite — they only compute what is pulled.
naturals := gen fn() {
  i := 0;
  while true { yield i; i = i + 1; };
};
Iter := import 'Iter';
print('first 6 =', naturals() |> Iter.take(6) |> Iter.collect());
// => [0, 1, 2, 3, 4, 5]
`,

  green: `// Green threads — 'go' starts a coroutine that runs cooperatively
// inside this actor; 'join' waits for one and returns its result.

// 'go' evaluates to a handle you can 'join' later.
total := go fn() {
  sum := 0;
  for (i, 1..=100) { sum = sum + i };
  sum
};
print('sum 1..100 =', join(total));          // => 5050

// Fan work out across many coroutines, then join them all back in.
handles := for[] (n, 1..=8) { go fn() { n * n } };
squares := for[] (h, handles) { join(h) };
print('squares =', squares);
// => [1, 4, 9, 16, 25, 36, 49, 64]
`,

  channels: `// A 'LocalChannel' passes values between green threads. 'recv' on an
// empty channel yields, so the producer is free to run and fill it.

LC := import 'LocalChannel';

ch := LC.new();
producer := go fn() {
  for (i, 1..=5) { LC.send(ch, i * 10) };
  LC.close(ch);
};

// Drain the channel until the producer closes it.
received := [];
draining := true;
while draining {
  msg := LC.recv(ch);
  if msg.closed == true {
    draining = false
  } else {
    received = received + [msg.value]
  }
};
join(producer);
print('received =', received);               // => [10, 20, 30, 40, 50]
`,

  errors: `// Errors are values — 'raise' throws any value, 'catch' binds exactly
// what was thrown, and built-in errors arrive as a structured object.

// A raised value comes back verbatim.
caught := try (raise \${code: 503, detail: 'upstream down'}) catch (e) { e };
print('code =', caught.code, ' detail =', caught.detail);
// => code = 503  detail = upstream down

// A built-in error reifies into \${kind, message, line}.
divide := fn(a, b) {
  try (a / b) catch (e) {
    print('  caught {e.kind}: {e.message}');
    0                          // fall back to a safe value
  }
};
print('10 / 2 =', divide(10, 2));              // => 5
print('10 / 0 =', divide(10, 0));              // => 0  (after the report)

// 'match' on e.kind handles different failures distinctly.
safe := fn(thunk) {
  try thunk() catch (e) {
    match e.kind {
      'div_by_zero'   => 'arithmetic error',
      'type_mismatch' => 'type error',
      _               => 'unexpected: {e}',
    }
  }
};
print(safe(fn() { 1 / 0 }));                   // => arithmetic error
print(safe(fn() { 1 + 'x' }));                 // => type error
`,
};
