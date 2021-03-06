# tigr
Basic language where everything is an expression

#Currently implemented:

##Types:

Int/Float : `6 | 9.8`

Strings (single quotes) : `'i am a string!'`

Bool : `true | false`

Arrays : `[1, 'string', true]`

Objects : `${ index: 'value', 'index2': 2 }`

Functions : `fn(argument) { argument * 2 }`

Null : `null`

##Arithmetic
The usual `+`, `-`, `/`, `*` and `%`

##Variable Assignment

`variable = 0;`

Also `+=`, `-=`, `/=`, `*=` and `%=`

##Comparisons

`==`, `!=`, `>`, `<`, `>=`, `<=`, `&&`, `||`

##Arrays
Arrays can be a mix of all types. Expressions can also be used.

Declaring:

```
foo = [1,2,3,4]
bar = [true, 8, 5 * 10, (a = 10; b = a; b * a)]  // becomes bar = [true, 8, 50, 100]
```

Accessing:

`foo[0]`

Array overloads the `+` operator,

Array + Expression will add the other evaluated expression to the end of the array.

`+=` can also be used on array variables

```
foo = [1,2,3];
foo += 4; //equivalent to foo = foo + 4 => foo == [1,2,3,4]
foo += [5,6]; // results in foo == [1,2,3,4,[5,6]]
```

##Objects
Objects are like arrays but with string indices instead of ints.

Declaring:

```
foo = ${};
foo2 = ${
    bar: 'string'
};
foo3 = ${
    bar: 'string',
    bar2: 34,
};
```

Then set or access:

```
foo.some_index = 10;
foo['some_index'] == 10 //true
```

##Functions
A function is defined with the keyword `fn` and has the form:

`fn (arguments) scope`

Where arguments are an optional comma separated list of variable names.

The function is then called in the usual way.

```
my_func = fn(a,b) { a + b }; //function returns the sum of the two arguments

my_func(1,2) //equals 3
```

Functions can return early using the `return` keyword, passing an optional value or expression.

Function run in the environment closure of when they are declared.

There is also currently no tail call optimisation, so recursive functions will VERY quickly overflow the stack.

Functions call be called by passing the `(arguments)` to any expression that equates to a function.

eg

```
a = fn() { 0 };
a(); //Can call it on ariable names

fn() { 0 }(); //can call it directly on the function definition

if false {
    fn() {
        return false //explicit return
    }
} else {
    fn() {
        true
    }
}(); //can call on if/for/while
```

##Blocks
Block of code are `;` seperated expressions.

A block resolves to the last expression in the block (or null if the last expression is ended in `;`

eg:

`(a=1;true;8)` is equal to `8`

`(b=3*4;false;)` is to `null`

Blocks are expressions too:

`a = (10 * (b = 8))` will set b to 8 and a to 80

##Scope
Scopes are just blocks surrounded in `{}`. They are also expressions.

Any new variables defined in the scope will not be visible outside the scope.

Changes to previously defined variables persist outside.

eg:

```
a = 9;
b = { (c = 20) * (a += 1) };
```

results, at the end of execution, in `a` being 10, `b` being 200 and c being undeclared.

##if
If takes the form:

`if expression scope else if scope else scope`

Where the else if and else branches are optional.
If resolves to the value of whichever branch's scope is excecuted (or null if there is no matching branch)

eg:

```
if true { 10 } // = 10
if false { 10 } // = null
if (a=10;b=100;a>b) { false } else if b == 99 { false } else { true } // = true
```
##for
For takes the form:

`for (enum?, iter?, range) scope`

where
- enum is a count of interations increasing by one each time, starting at 0
- iter is the value as specified by the range

both are optional.

range takes the form:

`from..to:step?`

step is optional and defaults to 1

so to count from 0 to 9 (inclusive) the range would be `0..10`

two count in 2s `0..10:2`

These can also be expressions, 

eg. 
```
a = 10; 
for (0..a:2) { ... } // = 0..10:2
for (5-2..(200;false;30):if true { 1 } else { 2 }) { ... } // = 3..30:1
```

There are two types of for loop:

`for` returning a single value corresponding to the value of the scope at the last iteration

`for[]` returning an array containing the value of the scope at each iteration.

eg.

```
a = for (i,0..10) { i };
b = for[] (e,i,0..10:2) { e };
```

results in `a == 9` and `b = [0,1,2,3,4]`

##while
While loop takes the form:

`while expression scope`

repeats scope until expression is false.

There are two types of while loop:

`while` returning a single value corresponding to the value of the scope at the last iteration

`while[]` returning an array containing the value of the scope at each iteration.

###break
Loops can be broken out early using the keyword ``break``

`break` can also take a value to return (default null)

This value can either be a literal type, or an expression in parenthesis.

eg:

```
for (i,0..10) {
    if i == 5 { break }
} // == null

for (i,0..10) {
    if i == 5 { break i }
} // == 5

while true {
    if true { break 6 * 7 } // error, expressions returned from break must be contained in ()
}

for (i,0..10) {
    for (j,0..10) {
        if i * j == 25 {
            break (break [i,j]) //breaks can be chained to break from outer loops!
        }
    }
} // == [5,5]
```
