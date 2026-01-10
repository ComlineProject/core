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
const MIN_VALUE: i8 = -128
```

**Supported Types:**
- Unsigned integers: `u8`, `u16`, `u32`, `u64`
- Signed integers: `i8`, `i16`, `i32`, `i64`
- Booleans: `bool`
- Strings: `str`, `string`

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
- Multiple args: `function process(str, u32, bool) returns i64`

## Type System

### Primitive Types

| Type | Description | Example |
|------|-------------|---------|
| `u8` - `u64` | Unsigned integers | `count: u32` |
| `i8` - `i64` | Signed integers | `offset: i32` |
| `f32`, `f64` | Floating point (partial support) | `ratio: f32` |
| `bool` | Boolean | `enabled: bool` |
| `str`, `string` | String | `name: str` |

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
- Optional types (`optional Type` or `Type?`)
- Annotations (`@required`, `@max=100`)
- Named function arguments
- Docstrings (parsed but not used)
- Error/exception types
- Default values for struct fields
- Union types

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
