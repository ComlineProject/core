# Comline IDL Grammar Reference

## Introduction

The Comline IDL (Interface Definition Language) uses a modern rust-sitter parser to define data structures, enums, and protocols for cross-language communication.

## Supported Declarations

### 1. Import

Import external modules or definitions:

```idl
import std
import my_module
```

### 2. Constants

Define compile-time constants with specific types:

```idl
const MAX_USERS: u32 = 1000
const API_VERSION: str = "v1.0"
const ENABLED: bool = true
const MIN_VALUE: s8 = -128
```

**Supported Types:**
- Unsigned integers: `u8`, `u16`, `u32`, `u64`
- Signed integers: `s8`, `s16`, `s32`, `s64`
- Booleans: `bool`
- Strings: `str`, `string`

The value can be a literal, a bare identifier (another constant), a
`::`-separated path (`u32::MIN`, `pkg::mod::DEFAULT`), or an f-string
(`f"page {N} of {total}"` - `{path}` placeholders, `{{`/`}}` escapes). Anything
other than a plain literal is recorded as text and not resolved to a value yet.
The same value forms work as struct-field and validator-property defaults.

### 3. Structs

Define data structures with typed fields:

```idl
struct User {
    id: u64
    name: str
    email: str
    active: bool
}
```

**With Arrays:**
```idl
struct Container {
    items: str[]           // Dynamic array
    buffer: u8[256]        // Fixed-size array
    data: CustomType[]     // Custom type arrays
}
```

**With Default Values:**
```idl
struct Config {
    retries: u32 = 3
    label: str = "default"
    optional timeout: s64 = -1
}
```
A default composes with `optional` and uses the same value grammar as
`const` (number, string, boolean, or an identifier/`::`-scoped reference -
though only a literal matching the field's own primitive type is actually
captured into the compiled schema today; anything else, like a reference
to another constant, parses but its value isn't resolved).

### 4. Enums

Define enumeration types with named variants:

```idl
enum Status {
    Active
    Inactive
    Pending
}

enum Color {
    Red
    Green
    Blue
}
```

### 5. Protocols

Define RPC-style service interfaces with functions:

```idl
protocol UserService {
    function getUser(u64) returns User
    function createUser(str, str) returns u64
    function listUsers() returns User[]
    function deleteUser(u64) returns bool
}
```

**Function Syntax:**
- `function NAME(ARG_TYPES...) returns RETURN_TYPE`
- No arguments: `function reset() returns bool`
- No return: `function notify(str)`  
- Multiple args: `function process(str, u32, bool) returns s64`
- Named args (optional): `function process(name: str, age: u32) returns bool`
  - a bare type and a `name: Type` pair can be mixed freely in the same
    argument list; the name (when given) is carried through to generated
    code instead of a synthesized `arg0`/`arg1`

### 6. Annotations

Attach `@key=value` metadata above a struct, a field, a protocol, or a
function - one annotation per line, stacked if there's more than one:

```idl
@indexed=true
struct Product {
    @deprecated=true
    legacy_sku: str

    id: u64
}
```

The value can be a scalar - a number, a string, or a bare identifier - or a
list of named calls with keyword arguments:

```idl
struct Message {
    @validators=[StringBounds(min_chars=3, max_chars=12)]
    recipient: str
}
```

Annotations are captured into the IR (as `FrozenUnit::Property` on the
declaration, list values normalised to text) but nothing acts on them yet -
they don't affect validation or generated code. `@validators` in particular is
recorded, not enforced (see the validators design note).

### 7. Docstrings

Attach documentation with `///` line comments (note: three slashes, not
two) directly above a `const`, `struct`, a field, `enum`, `protocol`, or
`function`. A docstring can be one line or several consecutive `///`
lines; each line is either a plain description or an `@name: description`
line documenting one field/argument:

```idl
/// Checks if a string length is within bounds.
/// @min_chars: Minimum length of the string.
struct StringBounds {
    min_chars: u32
}
```

A docstring composes with `optional`/a default value/annotations on a
field - when combined, it goes first:

```idl
struct Product {
    /// The item's stock keeping unit, retired in v2.
    @deprecated=true
    optional legacy_sku: str
}
```

Consecutive `///` lines are joined with newlines into one string and
stored as-is (the `@name:` form isn't parsed into separate per-parameter
data - it's just text). **A `///` line only parses immediately before
something that can carry a docstring** - unlike a plain `//` comment,
which can go anywhere and is silently discarded, a misplaced `///` (at
the end of a file, or before a declaration kind with no docstring slot,
like `use`/`import`) is a parse error. As with annotations, nothing in the
compiler reads a populated docstring yet - it's captured in the compiled
schema, but doesn't currently affect generated code.

### 8. Error Types

Declare a named error with `error NAME { message = "..." <fields> }`.
`message` is required - every error has one - and can be followed by zero
or more ordinary fields (annotations/`optional`/defaults all work on them,
same as struct fields):

```idl
error NotFoundError {
    message = "{self.id} was not found"
    id: u64
}
```

**Message interpolation:** a single brace substitutes a dotted-path value
- `{self.id}` above becomes whatever `id` is at the point the error is
raised. A **doubled** brace (`{{` / `}}`) is an escaped literal `{`/`}`
character instead - since a single brace always attempts interpolation,
write `{{404}}` for the literal text `{404}`, not `{404}` (which is a
parse error, since `404` isn't a valid path). The same interpolation is
available in an [f-string](#2-constants) value (`f"..."`); a plain `"..."`
string anywhere - `const`, annotation, field default - stays literal.

**Declaring that a function can throw one:** append `! ErrorName` after
the return type:

```idl
protocol UserService {
    function get(u64) -> User ! NotFoundError;
}
```

`! ErrorName` is a bare reference - the thrown error's fields aren't bound
to any of the function's arguments (no `! NotFoundError(some.expr)` form
yet), and the referenced name isn't checked against an actual declared
`error` - a typo there compiles without complaint today. A function can
declare at most one thrown error currently.

## Type System

### Primitive Types

| Type | Description | Example |
|------|-------------|---------|
| `u8` - `u64` | Unsigned integers | `count: u32` |
| `s8` - `s64` | Signed integers | `offset: s32` |
| `f32`, `f64` | Floating point (partial support) | `ratio: f32` |
| `bool` | Boolean | `enabled: bool` |
| `str`, `string` | String | `name: str` |

**Naming convention:** the leading letter is the sign, the number is the
bit width - `u` = unsigned (`u8`...`u64`), `s` = signed (`s8`...`s64`),
`f` = floating point (`f32`, `f64`). Many languages (C, Rust, Zig, ...)
spell signed integers with an `i` prefix instead - Comline uses `s`
deliberately: paired against `u`, `s`/`u` reads unambiguously as
signed/unsigned at a glance, whereas `i` more easily reads as just
"integer" and leaves the sign implicit. This is the only spelling the
grammar recognizes as a primitive keyword; code generators are
responsible for translating `s8`...`s64` to whatever their target
language natively calls a signed integer (e.g. the Rust generator emits
`i8`...`i64`, since that's Rust's own spelling, not Comline's).

### Custom Types

Reference user-defined types by name:

```idl
struct Message {
    sender: User        // Custom type
    status: Status      // Enum type
}
```

### Array Types

**Dynamic Arrays:**
```idl
items: str[]
users: User[]
```

**Fixed-Size Arrays:**
```idl
buffer: u8[256]
ids: u64[10]
```

**Nested Arrays (supported):**
```idl
matrix: u32[][]
```

## Syntax Rules

### Whitespace

Whitespace (spaces, tabs, newlines) is flexible:

```idl
// All valid:
struct User { name: str }
struct   User   {   name  :  str   }
struct User {
    name: str
}
```

### Comments

Single-line comments with `//`:

```idl
// This is a comment
import std // Inline comment

struct User { // Comment here
    name: str // And here
}
```

### Identifiers

- Start with letter or underscore
- Can contain letters, numbers, underscores
- Case-sensitive

```idl
struct MyType_123 { ... }  // ✅ Valid
struct _Private { ... }     // ✅ Valid
struct 123Invalid { ... }   // ❌ Invalid
```

## Complete Example

```idl
// User management system
import std

const MAX_USERS: u32 = 1000
const DEFAULT_ROLE: str = "user"

enum UserRole {
    Admin
    User
    Guest
}

enum Status {
    Active
    Inactive
    Suspended
}

struct User {
    id: u64
    username: str
    email: str
    role: UserRole
    status: Status
    tags: str[]
}

struct UserList {
    users: User[]
    total: u32
    page: u32
}

protocol UserService {
    function getUser(u64) returns User
    function createUser(str, str, UserRole) returns u64
    function listUsers(u32, u32) returns UserList
    function updateUser(u64, str) returns bool
    function deleteUser(u64) returns bool
    function searchUsers(str) returns User[]
}

protocol AuthService {
    function login(str, str) returns str
    function logout(str) returns bool
    function validateToken(str) returns bool
}
```

## Best Practices

1. **Use clear names**: Prefer `user_id` over `uid`
2. **Group related types**: Keep related structs/enums together
3. **Document complex types**: Use comments for non-obvious designs
4. **Consistent naming**: Choose a naming convention and stick to it
5. **Logical ordering**: Import → Constants → Types → Protocols

## Grammar Limitations

**Not Yet Supported:**
- Postfix optional type syntax (`Type?`) - the `optional` prefix keyword
  (`optional name: Type`) is supported (see Structs above)
- Argument-binding on a `throws` clause (`! ErrorName(some.expr)`) - a
  bare `! ErrorName` reference is supported (see Error Types above), and
  a function can only declare one thrown error
- Validating that a `! ErrorName` reference actually names a declared
  `error` - it's captured as plain text today, not resolved

**Coming Soon:**
These features are planned for future releases.

---

## Migration from Old Parser

If migrating from the old pest/lalrpop parser:

**Key Changes:**
- Whitespace handling improved
- Multi-declaration files now supported
- Array syntax added
- Negative numbers in constants supported
- More consistent error messages

**No Breaking Changes:**
All valid old IDL should parse correctly with the new parser.
