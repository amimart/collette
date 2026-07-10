# Collette

ℹ️ **coll**_ection_ + **ette** (i.e. meaning lightweight)

![Status](https://img.shields.io/badge/status-🚧%20WIP-yellow?style=for-the-badge)
[![lint](https://img.shields.io/github/actions/workflow/status/amimart/collette/lint.yaml?label=lint&style=for-the-badge&logo=github)](https://github.com/amimart/collette/actions/workflows/lint.yaml)
[![build](https://img.shields.io/github/actions/workflow/status/amimart/collette/build.yaml?label=build&style=for-the-badge&logo=github)](https://github.com/amimart/collette/actions/workflows/build.yaml)
[![test](https://img.shields.io/github/actions/workflow/status/amimart/collette/test.yaml?label=test&style=for-the-badge&logo=github)](https://github.com/amimart/collette/actions/workflows/test.yaml)

Typed collections, maintained indexes, and ordered scans over embedded key-value stores. Collette is:

- **Lightweight:** Collette adds structure to ordered KV storage through zero-cost abstractions.
- **Typed:** define Rust entities, primary keys, and indexes with compile-time checks.
- **Backend-agnostic:** storage is provided by pluggable multistore backends, while application code works with collections and scans.

Collette is not an ORM, query planner, SQL layer, or database server.

>🚧 **WARNING**: Collette is not mature enough to be considered production-grade. Its API may change without notice. But feedbacks are welcomed 😉

## Install

Add Collette to your `Cargo.toml`:

```toml
[dependencies]
collette = "0.1"
```

The in-memory backend is enabled by default. Enable `redb` for persistent embedded storage:

```toml
[dependencies]
collette = { version = "0.1", features = ["redb"] }
```

## Quick Start

```rust
use collette::backend::memory::InMemoryMultiStore;
use collette::{collection, CodecError, Entity};

fn main() -> Result<(), collette::Error> {
    let db = InMemoryMultiStore::new();

    let users = collection::<User, _>("users", db).build();

    users.save(User {
        id: 1,
        email: "ada@example.com".to_owned(),
        status: Status::Active,
        created_at: 1_700_000_000,
    })?;

    let _ada = users.get(1)?;

    Ok(())
}

struct User {
    id: u64,
    email: String,
    status: Status,
    created_at: u64,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
enum Status {
    Inactive,
    Active,
}

// Implement the Key trait for the Status enum
collette::impl_enum_key!(Status as u8 {
    Status::Inactive => 0,
    Status::Active => 1,
});

// Implement Entity for User so Collette can persist it.
impl Entity for User {
    type Key<'a> = u64;

    fn key(&self) -> Self::Key<'_> {
        self.id
    }

    fn to_bytes(&self) -> Result<Vec<u8>, CodecError> {
        let status = match self.status {
            Status::Inactive => "0",
            Status::Active => "1",
        };
        Ok(format!("{}|{}|{}|{}", self.id, self.created_at, status, self.email).into_bytes())
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        let text = String::from_utf8_lossy(bytes);
        let parts: Vec<_> = text.splitn(4, '|').collect();
        Ok(Self {
            id: parts[0].parse().unwrap(),
            created_at: parts[1].parse().unwrap(),
            status: match parts[2] {
                "0" => Status::Inactive,
                _ => Status::Active,
            },
            email: parts[3].to_owned(),
        })
    }
}
```

## Supported backends

| Backend | Feature | Use case |
| --- | --- | --- |
| In-memory | `memory` enabled by default | Tests, examples, and ephemeral in-process state. |
| redb | `redb` | Persistent embedded storage backed by [`redb`](https://crates.io/crates/redb). |

Application code should not call backend traits directly. Pick a backend, build a collection, and work through `Collection`.

## Indexes

Indexes are declared as Rust types implementing `Index<Entity>`.

Collette currently supports two index kinds:

### Unique

One record per index key. Useful for emails, names, handles, or other unique fields. For example:

```rust
struct ByEmail;

impl Index<User> for ByEmail {
    type Key<'a> = &'a str;
    type Kind<'a> = Unique;

    const NAME: &'static str = "by_email";

    fn key(user: &User) -> Self::Key<'_> {
        user.email.as_str()
    }
}
```

### Multi

Many records may share the same index key. Collette appends the primary key internally. For example:

```rust
struct ByStatus;

impl Index<User> for ByStatus {
    type Key<'a> = (Status,);
    type Kind<'a> = Multi;

    const NAME: &'static str = "by_status";

    fn key(user: &User) -> Self::Key<'_> {
        (user.status,)
    }
}
```

Registering indexes on the collection makes index scans compile-time checked:

```rust
let users = collection::<User, _>("users", db)
    .with_index::<ByEmail>()
    .with_index::<ByStatus>()
    .build();
```

## Query

Indexes are the query surface. A compound `Multi` index lets you group records by a prefix, then keep each group ordered by the next key parts.

```rust
struct ByStatusAndCreatedAt;

impl Index<User> for ByStatusAndCreatedAt {
    type Key<'a> = (Status, u64);
    type Kind<'a> = Multi;

    const NAME: &'static str = "by_status_and_created_at";

    fn key(user: &User) -> Self::Key<'_> {
        (user.status, user.created_at)
    }
}
```

Then scan it by prefix or by range with cursor support:

```rust
let active_users = users.scan(ByStatusAndCreatedAt)?
    .prefix(Status::Active)
    .direction(Direction::LeftToRight)
    .iter()?;

let recently_active_users = users.scan(ByStatusAndCreatedAt)?
    .prefix_range(
        Bound::Included((Status::Active, 1_700_000_000))
            ..Bound::Excluded((Status::Active, 1_800_000_000)),
    )
    .direction(Direction::LeftToRight)
    .iter()?;

let last_seen_user_id = 42;
let next_page = users.scan(ByStatusAndCreatedAt)?
    .prefix(Status::Active)
    .after((Status::Active, 1_700_010_000, &last_seen_user_id))
    .direction(Direction::LeftToRight)
    .iter()?;
```
