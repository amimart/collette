#![allow(dead_code)]

use colette::collection::{collection, Collection};
use colette::entity::Entity;
use colette::error::{CodecError, Error};
use colette::impl_enum_key;
use colette::index::{Index, Multi, Unique};
use colette::index_registry::{Cons, Nil};
use colette::key::{Key, KeySize};
use colette::scan::{Direction, IndexScan, PrefixScan};
use colette::store::{MultiStore, MultiStoreReadHandle};

pub fn run_collection_contract_tests<DB: MultiStore>(make_db: impl Fn() -> DB) {
    single_value_primary_key_behaviour(&make_db);
    tuple_primary_key_behaviour(&make_db);
    insert_rejects_duplicate_primary_key(&make_db);
    update_requires_existing_record(&make_db);
    update_replaces_existing_record(&make_db);
    save_inserts_new_record(&make_db);
    save_updates_existing_record(&make_db);
    remove_deletes_existing_record(&make_db);
    remove_missing_record_is_ok(&make_db);
    insert_get_remove_get_sequence(&make_db);
    unique_indexes_handle_and_scan_single_pair_and_triple_keys(&make_db);
    multi_indexes_handle_and_scan_single_pair_and_triple_keys(&make_db);
    scans_filter_lexicographically_with_ranges_directions_and_cursors(&make_db);
}

#[macro_export]
macro_rules! collection_contract_tests {
    ($make_db:expr) => {
        #[test]
        fn single_value_primary_key_behaviour() {
            $crate::common::single_value_primary_key_behaviour(&$make_db);
        }

        #[test]
        fn tuple_primary_key_behaviour() {
            $crate::common::tuple_primary_key_behaviour(&$make_db);
        }

        #[test]
        fn insert_rejects_duplicate_primary_key() {
            $crate::common::insert_rejects_duplicate_primary_key(&$make_db);
        }

        #[test]
        fn update_requires_existing_record() {
            $crate::common::update_requires_existing_record(&$make_db);
        }

        #[test]
        fn update_replaces_existing_record() {
            $crate::common::update_replaces_existing_record(&$make_db);
        }

        #[test]
        fn save_inserts_new_record() {
            $crate::common::save_inserts_new_record(&$make_db);
        }

        #[test]
        fn save_updates_existing_record() {
            $crate::common::save_updates_existing_record(&$make_db);
        }

        #[test]
        fn remove_deletes_existing_record() {
            $crate::common::remove_deletes_existing_record(&$make_db);
        }

        #[test]
        fn remove_missing_record_is_ok() {
            $crate::common::remove_missing_record_is_ok(&$make_db);
        }

        #[test]
        fn insert_get_remove_get_sequence() {
            $crate::common::insert_get_remove_get_sequence(&$make_db);
        }

        #[test]
        fn unique_indexes_handle_and_scan_single_pair_and_triple_keys() {
            $crate::common::unique_indexes_handle_and_scan_single_pair_and_triple_keys(&$make_db);
        }

        #[test]
        fn multi_indexes_handle_and_scan_single_pair_and_triple_keys() {
            $crate::common::multi_indexes_handle_and_scan_single_pair_and_triple_keys(&$make_db);
        }

        #[test]
        fn scans_filter_lexicographically_with_ranges_directions_and_cursors() {
            $crate::common::scans_filter_lexicographically_with_ranges_directions_and_cursors(
                &$make_db,
            );
        }
    };
}

pub fn single_value_primary_key_behaviour<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let users = user_collection("single_value_primary_key_behaviour", make_db());
    let ada = user(
        100,
        "ada",
        "ada@example.test",
        Region::Europe,
        AccountStatus::Active,
        Plan::Team,
        "core",
        1,
    );

    users.insert(&ada).unwrap();

    assert_eq!(users.get(100).unwrap(), Some(ada));
    assert_eq!(users.get(404).unwrap(), None);
}

pub fn tuple_primary_key_behaviour<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let memberships = membership_collection("tuple_primary_key_behaviour", make_db());
    let core_ada = membership("core", 100, Role::Owner, "founder");

    memberships.insert(&core_ada).unwrap();

    assert_eq!(
        memberships.get(("core".to_string(), 100)).unwrap(),
        Some(core_ada)
    );
    assert_eq!(memberships.get(("core".to_string(), 404)).unwrap(), None);
}

pub fn insert_rejects_duplicate_primary_key<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let users = user_collection("insert_rejects_duplicate_primary_key", make_db());
    let ada = sample_ada();
    let renamed = user(
        ada.id,
        "ada-renamed",
        "ada-renamed@example.test",
        Region::Pacific,
        AccountStatus::Suspended,
        Plan::Enterprise,
        "security",
        9,
    );

    users.insert(&ada).unwrap();

    assert!(matches!(
        users.insert(&renamed),
        Err(Error::AlreadyExists(_))
    ));
    assert_eq!(users.get(ada.id).unwrap(), Some(ada));
}

pub fn update_requires_existing_record<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let users = user_collection("update_requires_existing_record", make_db());

    assert!(matches!(
        users.update(sample_ada()),
        Err(Error::NotFound(_))
    ));
    assert_eq!(users.get(100).unwrap(), None);
}

pub fn update_replaces_existing_record<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let users = user_collection("update_replaces_existing_record", make_db());
    let ada = sample_ada();
    let moved = user(
        ada.id,
        "ada",
        "ada@example.test",
        Region::Americas,
        AccountStatus::Suspended,
        Plan::Enterprise,
        "security",
        4,
    );

    users.insert(&ada).unwrap();
    users.update(&moved).unwrap();

    assert_eq!(users.get(ada.id).unwrap(), Some(moved));
}

pub fn save_inserts_new_record<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let users = user_collection("save_inserts_new_record", make_db());
    let ada = sample_ada();

    users.save(&ada).unwrap();

    assert_eq!(users.get(ada.id).unwrap(), Some(ada));
}

pub fn save_updates_existing_record<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let users = user_collection("save_updates_existing_record", make_db());
    let ada = sample_ada();
    let changed = user(
        ada.id,
        "ada",
        "ada@example.test",
        Region::Europe,
        AccountStatus::Active,
        Plan::Team,
        "compiler",
        7,
    );

    users.save(&ada).unwrap();
    users.save(&changed).unwrap();

    assert_eq!(users.get(ada.id).unwrap(), Some(changed));
}

pub fn remove_deletes_existing_record<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let users = user_collection("remove_deletes_existing_record", make_db());
    let ada = sample_ada();

    users.insert(&ada).unwrap();
    users.remove(ada.id).unwrap();

    assert_eq!(users.get(ada.id).unwrap(), None);
}

pub fn remove_missing_record_is_ok<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let users = user_collection("remove_missing_record_is_ok", make_db());

    users.remove(404).unwrap();
    users.remove(404).unwrap();
}

pub fn insert_get_remove_get_sequence<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let users = user_collection("insert_get_remove_get_sequence", make_db());
    let ada = sample_ada();

    users.insert(&ada).unwrap();
    assert_eq!(users.get(ada.id).unwrap(), Some(ada.clone()));

    users.remove(ada.id).unwrap();
    assert_eq!(users.get(ada.id).unwrap(), None);
}

pub fn unique_indexes_handle_and_scan_single_pair_and_triple_keys<DB: MultiStore>(
    make_db: &impl Fn() -> DB,
) {
    let users = seeded_user_collection("unique_indexes_handle_and_scan", make_db());

    assert_eq!(
        scan_handles(users.scan(UniqueEmail).unwrap()),
        vec!["ada", "dennis", "grace", "linus", "margaret", "yukihiro"]
    );
    assert_eq!(
        scan_handles(
            users
                .scan(UniqueRegionHandle)
                .unwrap()
                .prefix(Region::Europe)
        ),
        vec!["ada", "grace", "linus"]
    );
    assert_eq!(
        scan_handles(
            users
                .scan(UniqueRegionPlanHandle)
                .unwrap()
                .prefix((Region::Europe, Plan::Team))
        ),
        vec!["ada"]
    );

    let duplicate_email = user(
        900,
        "duplicate-email",
        "ada@example.test",
        Region::Pacific,
        AccountStatus::Invited,
        Plan::Free,
        "support",
        1,
    );
    assert!(matches!(
        users.insert(&duplicate_email),
        Err(Error::AlreadyExists(_))
    ));
}

pub fn multi_indexes_handle_and_scan_single_pair_and_triple_keys<DB: MultiStore>(
    make_db: &impl Fn() -> DB,
) {
    let users = seeded_user_collection("multi_indexes_handle_and_scan", make_db());

    assert_eq!(
        scan_handles(users.scan(ByStatus).unwrap().prefix(AccountStatus::Active)),
        vec!["ada", "grace", "margaret"]
    );
    assert_eq!(
        scan_handles(
            users
                .scan(ByRegionStatus)
                .unwrap()
                .prefix((Region::Europe, AccountStatus::Active))
        ),
        vec!["ada", "grace"]
    );
    assert_eq!(
        scan_handles(users.scan(ByTeamStatusSeat).unwrap().prefix((
            "core",
            AccountStatus::Active,
            1u16
        ))),
        vec!["ada"]
    );
}

pub fn scans_filter_lexicographically_with_ranges_directions_and_cursors<DB: MultiStore>(
    make_db: &impl Fn() -> DB,
) {
    let users = seeded_user_collection("scan_ranges_directions_and_cursors", make_db());

    assert_eq!(
        scan_handles(users.scan(ByTeamStatusSeat).unwrap().prefix("core")),
        vec!["ada", "grace"]
    );
    assert_eq!(
        scan_handles(
            users
                .scan(ByTeamStatusSeat)
                .unwrap()
                .prefix("core")
                .direction(Direction::RightToLeft)
        ),
        vec!["grace", "ada"]
    );
    assert_eq!(
        scan_handles(
            users.scan(ByTeamStatusSeat).unwrap().prefix_range(
                std::ops::Bound::Included("core")..std::ops::Bound::Included("kernel")
            )
        ),
        vec!["ada", "grace", "linus"]
    );
    assert_eq!(
        scan_handles(
            users
                .scan(ByTeamStatusSeat)
                .unwrap()
                .prefix_range(
                    std::ops::Bound::Included("core")..std::ops::Bound::Included("kernel")
                )
                .direction(Direction::RightToLeft)
        ),
        vec!["linus", "grace", "ada"]
    );
    assert_eq!(
        scan_handles(users.scan(ByTeamStatusSeat).unwrap().range(
            std::ops::Bound::Included(("core", AccountStatus::Active, 1u16, &100u64))
                ..std::ops::Bound::Included(("core", AccountStatus::Active, 2u16, &101u64))
        )),
        vec!["ada", "grace"]
    );
    assert_eq!(
        scan_handles(
            users
                .scan(ByTeamStatusSeat)
                .unwrap()
                .range(
                    std::ops::Bound::Included(("core", AccountStatus::Active, 1u16, &100u64))
                        ..std::ops::Bound::Included(("core", AccountStatus::Active, 2u16, &101u64))
                )
                .direction(Direction::RightToLeft)
        ),
        vec!["grace", "ada"]
    );
    assert_eq!(
        scan_handles(users.scan(ByTeamStatusSeat).unwrap().prefix("core").after((
            "core",
            AccountStatus::Active,
            1u16,
            &100u64
        ))),
        vec!["grace"]
    );
    assert_eq!(
        scan_handles(
            users
                .scan(ByTeamStatusSeat)
                .unwrap()
                .prefix("core")
                .direction(Direction::RightToLeft)
                .after(("core", AccountStatus::Active, 2u16, &101u64))
        ),
        vec!["ada"]
    );
    assert!(matches!(
        users
            .scan(ByTeamStatusSeat)
            .unwrap()
            .prefix("core")
            .after(("kernel", AccountStatus::Suspended, 1u16, &102u64))
            .iter(),
        Err(Error::CursorOutOfBounds)
    ));
}

pub type UserCollection<DB> = Collection<
    DB,
    User,
    Cons<
        ByTeamStatusSeat,
        Cons<
            ByRegionStatus,
            Cons<
                ByStatus,
                Cons<UniqueRegionPlanHandle, Cons<UniqueRegionHandle, Cons<UniqueEmail, Nil>>>,
            >,
        >,
    >,
>;

pub type MembershipCollection<DB> = Collection<DB, Membership, Nil>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Americas,
    Europe,
    Pacific,
}

impl_enum_key!(Region as u8 {
    Region::Americas => 0,
    Region::Europe => 1,
    Region::Pacific => 2,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountStatus {
    Invited,
    Active,
    Suspended,
}

impl_enum_key!(AccountStatus as u8 {
    AccountStatus::Invited => 0,
    AccountStatus::Active => 1,
    AccountStatus::Suspended => 2,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    Free,
    Team,
    Enterprise,
}

impl_enum_key!(Plan as u8 {
    Plan::Free => 0,
    Plan::Team => 1,
    Plan::Enterprise => 2,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Maintainer,
    Viewer,
}

impl_enum_key!(Role as u8 {
    Role::Owner => 0,
    Role::Maintainer => 1,
    Role::Viewer => 2,
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: u64,
    pub handle: String,
    pub email: String,
    pub region: Region,
    pub status: AccountStatus,
    pub plan: Plan,
    pub team: String,
    pub seat: u16,
}

impl Entity for User {
    type Key<'a> = u64;

    fn key(&self) -> Self::Key<'_> {
        self.id
    }

    fn to_bytes(&self) -> Result<Vec<u8>, CodecError> {
        Ok(format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.id,
            self.handle,
            self.email,
            self.region.as_str(),
            self.status.as_str(),
            self.plan.as_str(),
            self.team,
            self.seat
        )
        .into_bytes())
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        let text = std::str::from_utf8(bytes).unwrap();
        let fields = text.split('|').collect::<Vec<_>>();
        assert_eq!(fields.len(), 8, "malformed user fixture: {text}");

        Ok(Self {
            id: fields[0].parse().unwrap(),
            handle: fields[1].to_string(),
            email: fields[2].to_string(),
            region: Region::parse(fields[3]),
            status: AccountStatus::parse(fields[4]),
            plan: Plan::parse(fields[5]),
            team: fields[6].to_string(),
            seat: fields[7].parse().unwrap(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Membership {
    pub org: String,
    pub user_id: u64,
    pub role: Role,
    pub label: String,
}

impl Entity for Membership {
    type Key<'a> = (&'a str, u64);

    fn key(&self) -> Self::Key<'_> {
        (self.org.as_str(), self.user_id)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, CodecError> {
        Ok(format!(
            "{}|{}|{}|{}",
            self.org,
            self.user_id,
            self.role.as_str(),
            self.label
        )
        .into_bytes())
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        let text = std::str::from_utf8(bytes).unwrap();
        let fields = text.split('|').collect::<Vec<_>>();
        assert_eq!(fields.len(), 4, "malformed membership fixture: {text}");

        Ok(Self {
            org: fields[0].to_string(),
            user_id: fields[1].parse().unwrap(),
            role: Role::parse(fields[2]),
            label: fields[3].to_string(),
        })
    }
}

pub struct UniqueEmail;
pub struct UniqueRegionHandle;
pub struct UniqueRegionPlanHandle;
pub struct ByStatus;
pub struct ByRegionStatus;
pub struct ByTeamStatusSeat;

impl Index<User> for UniqueEmail {
    type Key<'a> = &'a str;
    type Kind<'a> = Unique;
    const NAME: &'static str = "unique_email";

    fn key(entity: &User) -> Self::Key<'_> {
        entity.email.as_str()
    }
}

impl Index<User> for UniqueRegionHandle {
    type Key<'a> = (Region, &'a str);
    type Kind<'a> = Unique;
    const NAME: &'static str = "unique_region_handle";

    fn key(entity: &User) -> Self::Key<'_> {
        (entity.region, entity.handle.as_str())
    }
}

impl Index<User> for UniqueRegionPlanHandle {
    type Key<'a> = (Region, Plan, &'a str);
    type Kind<'a> = Unique;
    const NAME: &'static str = "unique_region_plan_handle";

    fn key(entity: &User) -> Self::Key<'_> {
        (entity.region, entity.plan, entity.handle.as_str())
    }
}

impl Index<User> for ByStatus {
    type Key<'a> = (AccountStatus,);
    type Kind<'a> = Multi;
    const NAME: &'static str = "by_status";

    fn key(entity: &User) -> Self::Key<'_> {
        (entity.status,)
    }
}

impl Index<User> for ByRegionStatus {
    type Key<'a> = (Region, AccountStatus);
    type Kind<'a> = Multi;
    const NAME: &'static str = "by_region_status";

    fn key(entity: &User) -> Self::Key<'_> {
        (entity.region, entity.status)
    }
}

impl Index<User> for ByTeamStatusSeat {
    type Key<'a> = (&'a str, AccountStatus, u16);
    type Kind<'a> = Multi;
    const NAME: &'static str = "by_team_status_seat";

    fn key(entity: &User) -> Self::Key<'_> {
        (entity.team.as_str(), entity.status, entity.seat)
    }
}

pub fn user_collection<DB: MultiStore>(name: &'static str, db: DB) -> UserCollection<DB> {
    db.prepare(
        name,
        [
            "__main",
            UniqueEmail::NAME,
            UniqueRegionHandle::NAME,
            UniqueRegionPlanHandle::NAME,
            ByStatus::NAME,
            ByRegionStatus::NAME,
            ByTeamStatusSeat::NAME,
        ],
    )
    .unwrap();

    collection::<User, DB>(name, db)
        .with_index::<UniqueEmail>()
        .with_index::<UniqueRegionHandle>()
        .with_index::<UniqueRegionPlanHandle>()
        .with_index::<ByStatus>()
        .with_index::<ByRegionStatus>()
        .with_index::<ByTeamStatusSeat>()
        .build()
}

pub fn membership_collection<DB: MultiStore>(
    name: &'static str,
    db: DB,
) -> MembershipCollection<DB> {
    db.prepare(name, ["__main"]).unwrap();
    collection::<Membership, DB>(name, db).build()
}

pub fn user(
    id: u64,
    handle: &str,
    email: &str,
    region: Region,
    status: AccountStatus,
    plan: Plan,
    team: &str,
    seat: u16,
) -> User {
    User {
        id,
        handle: handle.to_string(),
        email: email.to_string(),
        region,
        status,
        plan,
        team: team.to_string(),
        seat,
    }
}

pub fn membership(org: &str, user_id: u64, role: Role, label: &str) -> Membership {
    Membership {
        org: org.to_string(),
        user_id,
        role,
        label: label.to_string(),
    }
}

fn seeded_user_collection<DB: MultiStore>(name: &'static str, db: DB) -> UserCollection<DB> {
    let users = user_collection(name, db);
    for user in sample_users() {
        users.insert(user).unwrap();
    }
    users
}

fn sample_users() -> Vec<User> {
    vec![
        sample_ada(),
        user(
            101,
            "grace",
            "grace@example.test",
            Region::Europe,
            AccountStatus::Active,
            Plan::Enterprise,
            "core",
            2,
        ),
        user(
            102,
            "linus",
            "linus@example.test",
            Region::Europe,
            AccountStatus::Suspended,
            Plan::Free,
            "kernel",
            1,
        ),
        user(
            103,
            "margaret",
            "margaret@example.test",
            Region::Americas,
            AccountStatus::Active,
            Plan::Enterprise,
            "apollo",
            1,
        ),
        user(
            104,
            "yukihiro",
            "yukihiro@example.test",
            Region::Pacific,
            AccountStatus::Invited,
            Plan::Team,
            "ruby",
            3,
        ),
        user(
            105,
            "dennis",
            "dennis@example.test",
            Region::Americas,
            AccountStatus::Suspended,
            Plan::Team,
            "unix",
            2,
        ),
    ]
}

fn scan_handles<'a, ReadHandle, Idx>(scan: IndexScan<'a, ReadHandle, User, Idx>) -> Vec<String>
where
    ReadHandle: MultiStoreReadHandle,
    Idx: Index<User>,
    for<'b> Idx::Kind<'b>: colette::index::IndexKind<Idx::Key<'b>, <User as Entity>::Key<'b>>,
{
    scan.iter()
        .unwrap()
        .map(|entry| entry.unwrap().record.handle)
        .collect()
}

fn sample_ada() -> User {
    user(
        100,
        "ada",
        "ada@example.test",
        Region::Europe,
        AccountStatus::Active,
        Plan::Team,
        "core",
        1,
    )
}

impl Region {
    fn as_str(self) -> &'static str {
        match self {
            Self::Americas => "americas",
            Self::Europe => "europe",
            Self::Pacific => "pacific",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "americas" => Self::Americas,
            "europe" => Self::Europe,
            "pacific" => Self::Pacific,
            _ => panic!("unknown region: {value}"),
        }
    }
}

impl AccountStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Invited => "invited",
            Self::Active => "active",
            Self::Suspended => "suspended",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "invited" => Self::Invited,
            "active" => Self::Active,
            "suspended" => Self::Suspended,
            _ => panic!("unknown account status: {value}"),
        }
    }
}

impl Plan {
    fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Team => "team",
            Self::Enterprise => "enterprise",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "free" => Self::Free,
            "team" => Self::Team,
            "enterprise" => Self::Enterprise,
            _ => panic!("unknown plan: {value}"),
        }
    }
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Maintainer => "maintainer",
            Self::Viewer => "viewer",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "owner" => Self::Owner,
            "maintainer" => Self::Maintainer,
            "viewer" => Self::Viewer,
            _ => panic!("unknown role: {value}"),
        }
    }
}
