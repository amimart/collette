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

### Collection definition

```rust
let downloads = collection::<Download, DB>("downloads", DB {})
    .with_index::<UniqueName>()
    .with_index::<ByStatus>()
    .with_index::<ByStatusAndSize>()
    .build();

downloads.save(dl)?;
let my_dl = downloads.get(&dl.info_hash)?;
```

### Index scans

```rust
downloads.index(ByStatusAndSize).?
    .prefix_range(
        Bound::Included((Status::InProgress, 0))..Bound::Excluded((Status::InProgress, 1000000))
    ).direction(Direction::LeftToRight)
    .after(cursor)
    .iter();
```

### Entity definition

```rust
pub struct Download {
    id: InfoHash,
    name: String,
    status: Status,
    size: u64,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct InfoHash([u8; 20]);

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum Status {
    Queued,
    Submitted,
    InProgress,
    Completed,
}

impl_enum_key!(Status as u8 {
    Status::Queued => 0,
    Status::Submitted => 1,
    Status::InProgress => 2,
    Status::Completed => 3,
});

impl Entity for Download {
    type Key<'a>
        = &'a [u8; 20] // key extraction is zero-copy, encoding is zero-alloc
    where
        Self: 'a;

    fn key(&self) -> Self::Key<'_> {
        &self.id.0
    }

    fn to_bytes(&self) -> Result<Vec<u8>, CodecError> {
        todo!()
    }

    fn from_bytes(_bytes: &[u8]) -> Result<Self, CodecError> {
        todo!()
    }
}
```

### Index definition

```rust
pub struct UniqueName;
impl Index<Download> for UniqueName {
    type Key<'a> = &'a str; // key extraction is zero-copy, encoding may be zero-alloc
    type Kind<'a> = Unique;
    const NAME: &'static str = "name";

    fn key(entity: &Download) -> Self::Key<'_> {
        entity.name.as_str()
    }
}

pub struct ByStatus;
impl Index<Download> for ByStatus {
    type Key<'a> = (Status,); // key extraction is copy, encoding is zero-alloc
    type Kind<'a> = Multi;
    const NAME: &'static str = "status";

    fn key(entity: &Download) -> Self::Key<'_> {
        (entity.status,)
    }
}

pub struct ByStatusAndSize;
impl Index<Download> for ByStatusAndSize {
    type Key<'a> = (Status, u64); // key extraction is copy, encoding may be zero-alloc
    type Kind<'a> = Multi;
    const NAME: &'static str = "status_size";

    fn key(entity: &Download) -> Self::Key<'_> {
        (entity.status, entity.size)
    }
}
```
