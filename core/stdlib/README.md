# Comline Standard Library

This directory contains the standard library schemas available to all Comline packages via `use std::` imports.

## Structure

```
stdlib/
├── collections/    # Data structures
│   ├── HashMap.ids
│   └── Vec.ids
├── http/          # HTTP types
│   ├── Request.ids
│   └── Response.ids
├── time/          # Time-related types (TODO)
└── io/            # I/O operations (TODO)
```

## Usage

Import from standard library in your schemas:

```rust
use std::collections::HashMap;
use std::http::{Request, Response};

struct Cache {
    data: HashMap<string, bytes>
}
```

## Types

### collections::HashMap<K, V>
Generic key-value map with efficient lookups.

### collections::Vec<T>
Dynamic array with automatic resizing.

### http::Request
HTTP request with method, URI, headers, and body.

### http::Response
HTTP response with status code, headers, and body.
